//! Negotiation, the guard that keeps a strong validator honest, and a body
//! that fails while it is buffered.

use super::{Coding, Negotiated, negotiate, strongly_tagged};
use crate::http::{HeaderMap, HeaderValue, header};

/// A request accepting `value`, or accepting nothing at all.
fn accepting(value: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Some(value) = value {
        headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_str(value).expect("a printable field"),
        );
    }
    headers
}

/// Every rule RFC 9110 section 12.5.3 states, and what each resolves to.
///
/// One table, because the rules interact: identity's default acceptability
/// is what decides two of the rows, and the wildcard's reach decides two
/// more.
#[test]
fn every_negotiation_rule_the_specification_states_is_applied() {
    let cases: &[(&str, Option<&str>, Negotiated)] = &[
        // Rule 1: absent means everything is acceptable, identity included.
        ("no field at all", None, Negotiated::Unstated),
        // An empty value "implies that the user agent does not want any
        // content coding in response" -- it excludes nothing, so identity.
        ("an empty field value", Some(""), Negotiated::Identity),
        (
            "a plain coding",
            Some("gzip"),
            Negotiated::Encode(Coding::Gzip),
        ),
        (
            "the deprecated spelling of one",
            Some("x-gzip"),
            Negotiated::Encode(Coding::Gzip),
        ),
        (
            "a coding in another case",
            Some("GZIP"),
            Negotiated::Encode(Coding::Gzip),
        ),
        // Server preference breaks a tie: zstd is preferred over gzip.
        (
            "two codings weighted equally",
            Some("gzip, zstd"),
            Negotiated::Encode(Coding::Zstd),
        ),
        // The client's weighting overrides the server's preference.
        (
            "a client preferring the server's second choice",
            Some("gzip;q=1.0, zstd;q=0.5"),
            Negotiated::Encode(Coding::Gzip),
        ),
        // Rule 2, explicit form.
        (
            "identity refused by name",
            Some("gzip, identity;q=0"),
            Negotiated::Encode(Coding::Gzip),
        ),
        // Rule 2, wildcard form -- the one an implementation misses.
        (
            "identity refused through the wildcard",
            Some("gzip, *;q=0"),
            Negotiated::Encode(Coding::Gzip),
        ),
        // A more specific identity entry beats the wildcard.
        (
            "a wildcard refusal with identity readmitted",
            Some("*;q=0, identity"),
            Negotiated::Identity,
        ),
        (
            "every coding refused",
            Some("gzip;q=0, br;q=0, zstd;q=0"),
            Negotiated::Identity,
        ),
        // Nothing left at all: this is the 406.
        ("everything refused", Some("*;q=0"), Negotiated::Nothing),
        // With no wildcard, a coding the field does not list is unacceptable
        // too, so excluding identity alone leaves nothing.
        (
            "identity alone refused",
            Some("identity;q=0"),
            Negotiated::Nothing,
        ),
        // A wildcard weight that is not a qvalue refuses every coding, but it
        // is not the explicit `*;q=0` rule 2 needs to exclude identity.
        (
            "a wildcard weight above 1",
            Some("*;q=1.5"),
            Negotiated::Identity,
        ),
        (
            "a wildcard weight of infinity",
            Some("*;q=inf"),
            Negotiated::Identity,
        ),
        (
            "a wildcard weight that is not a number",
            Some("*;q=NaN"),
            Negotiated::Identity,
        ),
        // Nor is an identity weight that is not a qvalue its explicit
        // `identity;q=0`, even beside a wildcard refusal.
        (
            "an identity weight above 1",
            Some("identity;q=1.5"),
            Negotiated::Identity,
        ),
        (
            "an identity weight above 1 beside a wildcard refusal",
            Some("identity;q=1.5, *;q=0"),
            Negotiated::Identity,
        ),
        (
            "every coding and identity refused by name",
            Some("gzip;q=0, br;q=0, zstd;q=0, identity;q=0"),
            Negotiated::Nothing,
        ),
        // A client preferring identity gets it.
        (
            "identity preferred over a coding",
            Some("gzip;q=0.5, identity;q=1.0"),
            Negotiated::Identity,
        ),
    ];

    for (description, accept, expected) in cases {
        assert_eq!(negotiate(&accepting(*accept)), *expected, "{description}");
    }
}

/// A weight above 1 is not a qvalue and must not outrank one.
///
/// RFC 9110 section 12.4.2 bounds it at 1. Read literally, `q=1.5` beats a
/// legitimate `q=1.0` -- a preference inversion no client can have meant.
///
/// Its refusal is asserted where the weight is read, in
/// [`http::coding`](crate::http::coding); what belongs here is the outcome it
/// produces for *this* interceptor's choice among the codings it can produce.
#[test]
fn a_weight_outside_the_range_cannot_outrank_one_inside_it() {
    assert_eq!(
        negotiate(&accepting(Some("gzip;q=1.5, zstd;q=1.0"))),
        Negotiated::Encode(Coding::Zstd)
    );
}

fn tagged(value: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Some(value) = value {
        headers.insert(
            header::ETAG,
            HeaderValue::from_str(value).expect("a printable field"),
        );
    }
    headers
}

/// A strong tag stops the encoder; a weak one does not.
///
/// RFC 9110 section 8.8.1: a validator shared by a coded and an uncoded
/// representation *is* weak, so a response that already says `W/` is
/// telling the truth after encoding and one that does not is not.
#[test]
fn only_a_strong_validator_stops_the_encoder() {
    let cases: &[(&str, Option<&str>, bool)] = &[
        ("no validator at all", None, false),
        ("a strong tag", Some("\"rev-42\""), true),
        ("a weak tag", Some("W/\"rev-42\""), false),
        // Lowercase `w/` is not the weakness prefix: RFC 9110 section 8.8.3
        // writes it `W/`, case-sensitively.
        ("a lowercase weakness prefix", Some("w/\"rev-42\""), true),
        (
            "a strong tag with surrounding space",
            Some("  \"rev-42\"  "),
            true,
        ),
    ];

    for (description, tag, expected) in cases {
        assert_eq!(strongly_tagged(&tagged(*tag)), *expected, "{description}");
    }
}

/// Every zstd level, on both encoding paths, inside the window HTTP allows.
mod zstd_window {
    use async_compression::{tokio::bufread::ZstdDecoder, zstd::DParameter};
    use bytes::Bytes;
    use http_body_util::BodyExt as _;
    use tokio::io::AsyncReadExt as _;

    use crate::{
        http::body::Body,
        middleware::compression::{
            Coding, Levels, encode,
            levels::ZstdLevel,
            streaming::{LatencyMode, Streamed},
        },
    };

    /// RFC 9659 section 3's 8 MB, as the base-2 logarithm zstd states it in.
    const HTTP_WINDOW_LOG: u32 = 23;

    /// A body large enough for every level to reach its matching strategy.
    fn body() -> Bytes {
        use std::fmt::Write as _;

        let mut body = String::new();
        for line in 0..4_096 {
            writeln!(
                body,
                "line {line} of a response, {}",
                line * 7_919 % 104_729
            )
            .expect("writing to a String");
        }
        body.into()
    }

    /// What a decoder that refuses windows above 8 MB makes of `encoded`.
    async fn decoded_within_the_window(encoded: &[u8]) -> std::io::Result<Vec<u8>> {
        let mut decoded = Vec::new();
        ZstdDecoder::with_params(
            std::io::Cursor::new(encoded),
            &[DParameter::window_log_max(HTTP_WINDOW_LOG)],
        )
        .read_to_end(&mut decoded)
        .await?;
        Ok(decoded)
    }

    /// RFC 9659 section 3: an encoder "MUST NOT generate frames requiring a
    /// `Window_Size` larger than 8 MB". zstd's own table gives levels 20 to 22
    /// a 32 to 128 MB window when the input's size is unknown, which it is
    /// to both paths.
    ///
    /// Swept rather than sampled, so a zstd release that moves a level's
    /// window fails here rather than on a client.
    #[tokio::test]
    async fn every_zstd_level_decodes_within_the_window_rfc_9659_allows() {
        for level in 1..=22 {
            let levels = Levels {
                zstd: ZstdLevel::new(level).expect("a level zstd defines"),
                ..Levels::default()
            };

            let buffered = encode(Coding::Zstd, body(), levels)
                .await
                .expect("an encodable body");
            let streamed = Streamed::new(
                Body::from_bytes(body()),
                Coding::Zstd,
                levels,
                LatencyMode::Throughput,
            )
            .collect()
            .await
            .expect("an encodable body")
            .to_bytes();

            for (path, encoded) in [("buffered", buffered), ("streamed", streamed)] {
                let decoded = decoded_within_the_window(&encoded)
                    .await
                    .unwrap_or_else(|error| panic!("level {level}, {path}: {error}"));
                assert_eq!(decoded, body(), "level {level}, {path}");
            }
        }
    }
}

/// A response body that fails while `Compression` buffers it.
///
/// In-crate because no public surface builds a body that fails on demand:
/// `Body::from_body` is `pub(crate)`. Nor does one reach this path through
/// `Served`, whose `Accept-Ranges` makes `Compression` leave it alone.
mod failing {
    use std::convert::Infallible;

    use http_body_util::BodyExt;

    use crate::{
        Router,
        extract::body::text::Text,
        http::{Request, body::Body, header},
        middleware::{Continued, Interceptor, Next, compression::Compression, testing::Failing},
        openapi::{Method, PathTemplate},
        router::{endpoint::builder::EndpointBuilder, service::Service},
    };

    /// Replaces whatever the handler produced with a body stating 4096
    /// octets, yielding 1024 of them, and then failing.
    struct FailPartWay;

    impl Interceptor<()> for FailPartWay {
        type Reads = ();
        type Adds = ();
        type Short = Infallible;

        async fn intercept(
            &self,
            request: Request,
            (): (),
            (): &(),
            next: Next<'_, ()>,
        ) -> Result<Continued, Infallible> {
            let mut continued = next.run(request).await;
            drop(continued.take_body());
            continued.set_body(Body::from_body(
                Failing::after(vec![b'a'; 1024]).stating(4096),
            ));
            Ok(continued)
        }
    }

    async fn page() -> Text {
        Text("x".repeat(4096))
    }

    /// The page with its body replaced by a failing one, beneath `Compression`
    /// when `compressed`.
    fn service(compressed: bool) -> Service<()> {
        let endpoint = EndpointBuilder::new(
            Method::Get,
            PathTemplate::parse("/page").expect("a valid path"),
            page,
        );
        let router = Router::<()>::new().mount(endpoint);
        if compressed {
            router
                .intercept(Compression::new())
                .intercept(FailPartWay)
                .build(())
        } else {
            router.intercept(FailPartWay).build(())
        }
        .expect("a describable router")
    }

    /// Whether reading the response body to its end failed.
    async fn read_fails(service: &Service<()>) -> bool {
        let request = http::Request::builder()
            .method("GET")
            .uri("/page")
            .header(header::ACCEPT_ENCODING, "gzip")
            .body(Body::empty())
            .expect("a well-formed request");
        service
            .call(request)
            .await
            .into_body()
            .collect()
            .await
            .is_err()
    }

    /// A read that fails part-way is handed on failing, as it is with nothing
    /// encoding it, rather than as a complete, empty body.
    #[tokio::test]
    async fn a_body_that_fails_part_way_is_handed_on_failing() {
        assert!(
            read_fails(&service(false)).await,
            "the control read succeeded"
        );
        assert!(
            read_fails(&service(true)).await,
            "a failed read reached the client as a complete body"
        );
    }
}

/// What `Compression` sends, read as a client reads it.
mod wire {
    use std::{
        convert::Infallible,
        pin::Pin,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        task::{Context, Poll},
    };

    use async_compression::tokio::bufread::{BrotliDecoder, GzipDecoder, ZstdDecoder};
    use bytes::Bytes;
    use http_body::{Frame, SizeHint};
    use http_body_util::BodyExt;
    use tokio::io::AsyncReadExt as _;

    use crate::{
        Router,
        extract::body::text::Text,
        http::{
            Request, Response, StatusCode,
            body::{Body, BoxError},
            header,
        },
        middleware::{
            Continued, Interceptor, Next,
            compression::{
                Coding, Compression, Levels, encode,
                streaming::{LatencyMode, Streamed},
            },
        },
        openapi::{Method, PathTemplate},
        router::{endpoint::builder::EndpointBuilder, service::Service},
    };

    /// What the handler sends: long and repetitive enough for every coding to
    /// shrink it.
    fn page_text() -> String {
        "a line of the page the handler sends\n".repeat(256)
    }

    async fn page() -> Text {
        Text(page_text())
    }

    /// What a client decoding `encoded` under `coding` reads, through each
    /// coding's own decoder rather than anything `Compression` shares.
    async fn decoded(coding: Coding, encoded: &[u8]) -> Vec<u8> {
        let input = std::io::Cursor::new(encoded);
        let mut decoded = Vec::new();
        match coding {
            Coding::Gzip => GzipDecoder::new(input).read_to_end(&mut decoded).await,
            Coding::Brotli => BrotliDecoder::new(input).read_to_end(&mut decoded).await,
            Coding::Zstd => ZstdDecoder::new(input).read_to_end(&mut decoded).await,
        }
        .unwrap_or_else(|error| panic!("{coding:?} did not decode: {error}"));
        decoded
    }

    /// Hands on another body's frames while stating no length, which is what
    /// sends `Compression` down its streaming path.
    struct Unsized(Body);

    impl http_body::Body for Unsized {
        type Data = Bytes;
        type Error = BoxError;

        fn poll_frame(
            mut self: Pin<&mut Self>,
            context: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
            Pin::new(&mut self.0).poll_frame(context)
        }

        fn size_hint(&self) -> SizeHint {
            SizeHint::default()
        }
    }

    /// Records that the chain beneath `Compression` ran, and makes the body
    /// it produced unsized when `unsize` says so.
    struct Beneath {
        ran: Arc<AtomicBool>,
        unsize: bool,
    }

    impl Interceptor<()> for Beneath {
        type Reads = ();
        type Adds = ();
        type Short = Infallible;

        async fn intercept(
            &self,
            request: Request,
            (): (),
            (): &(),
            next: Next<'_, ()>,
        ) -> Result<Continued, Infallible> {
            self.ran.store(true, Ordering::SeqCst);
            let mut continued = next.run(request).await;
            if self.unsize {
                let body = continued.take_body();
                continued.set_body(Body::from_body(Unsized(body)));
            }
            Ok(continued)
        }
    }

    /// The page under `Compression`, its body unsized when `unsize`, with the
    /// flag the chain beneath sets when it runs.
    fn service(unsize: bool) -> (Service<()>, Arc<AtomicBool>) {
        let ran = Arc::new(AtomicBool::new(false));
        let endpoint = EndpointBuilder::new(
            Method::Get,
            PathTemplate::parse("/page").expect("a valid path"),
            page,
        );
        let service = Router::<()>::new()
            .mount(endpoint)
            .intercept(Compression::new())
            .intercept(Beneath {
                ran: Arc::clone(&ran),
                unsize,
            })
            .build(())
            .expect("a describable router");
        (service, ran)
    }

    /// `GET /page` accepting `accept`.
    async fn fetch(service: &Service<()>, accept: &str) -> Response {
        let request = http::Request::builder()
            .method("GET")
            .uri("/page")
            .header(header::ACCEPT_ENCODING, accept)
            .body(Body::empty())
            .expect("a well-formed request");
        service.call(request).await
    }

    /// The buffered encoder and the streaming one each produce, for every
    /// coding, a stream that coding's own decoder reads back to the body.
    #[tokio::test]
    async fn every_coding_decodes_to_the_body_on_both_encoding_paths() {
        let body = Bytes::from(page_text());

        for coding in Coding::ALL {
            let buffered = encode(coding, body.clone(), Levels::default())
                .await
                .expect("an encodable body");
            let streamed = Streamed::new(
                Body::from_bytes(body.clone()),
                coding,
                Levels::default(),
                LatencyMode::Interactive,
            )
            .collect()
            .await
            .expect("an encodable body")
            .to_bytes();

            for (path, encoded) in [("buffered", buffered), ("streamed", streamed)] {
                assert!(
                    encoded.len() < body.len(),
                    "{coding:?}, {path}: not smaller"
                );
                assert_eq!(decoded(coding, &encoded).await, body, "{coding:?}, {path}");
            }
        }
    }

    /// Each coding a client names is the one the response carries, labelled
    /// with its token, and decodes to the page: with the encoded length stated
    /// when the body's length was known, and with none when it streamed.
    #[tokio::test]
    async fn each_named_coding_reaches_the_wire_labelled_and_decodable() {
        for unsize in [false, true] {
            let (service, _) = service(unsize);

            for (token, coding) in [
                ("gzip", Coding::Gzip),
                ("br", Coding::Brotli),
                ("zstd", Coding::Zstd),
            ] {
                let response = fetch(&service, token).await;
                let case = format!("{token}, streamed: {unsize}");

                assert_eq!(response.status(), StatusCode::OK, "{case}");
                assert_eq!(
                    response
                        .headers()
                        .get_all(header::CONTENT_ENCODING)
                        .iter()
                        .collect::<Vec<_>>(),
                    [token],
                    "{case}"
                );
                let stated = response
                    .headers()
                    .get(header::CONTENT_LENGTH)
                    .map(|value| value.to_str().expect("a printable length").to_owned());
                let body = response
                    .into_body()
                    .collect()
                    .await
                    .expect("a complete body")
                    .to_bytes();

                assert_eq!(stated, (!unsize).then(|| body.len().to_string()), "{case}");
                assert_eq!(
                    decoded(coding, &body).await,
                    page_text().as_bytes(),
                    "{case}"
                );
            }
        }
    }

    /// RFC 9110 section 12.4.1: a request refusing every coding and identity
    /// too is answered 406 by `Compression` itself, before anything beneath it
    /// runs, with the problem document its declaration names.
    #[tokio::test]
    async fn a_request_refusing_every_coding_and_identity_is_refused_before_the_chain_runs() {
        let (service, ran) = service(false);
        let response = fetch(&service, "*;q=0, identity;q=0").await;

        assert_eq!(response.status(), StatusCode::NOT_ACCEPTABLE);
        assert!(
            !ran.load(Ordering::SeqCst),
            "the chain beneath ran for a request nothing could answer"
        );
        assert_eq!(response.headers().get(header::CONTENT_ENCODING), None);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .map(http::HeaderValue::as_bytes),
            Some(b"application/problem+json".as_slice())
        );
        let body = response
            .into_body()
            .collect()
            .await
            .expect("a complete body")
            .to_bytes();
        let problem: serde_json::Value =
            serde_json::from_slice(&body).expect("a JSON problem document");
        assert_eq!(
            problem,
            serde_json::json!({
                "type": "about:blank",
                "title": "Not Acceptable",
                "status": 406,
                "detail": "no representation of this resource has an acceptable content coding",
            })
        );
    }

    /// The control, differing in exactly what the request refuses: identity
    /// admitted by name beside the same wildcard refusal, so the chain runs
    /// and the page is sent as it is.
    #[tokio::test]
    async fn a_request_readmitting_identity_runs_the_chain_and_is_sent_identity() {
        let (service, ran) = service(false);
        let response = fetch(&service, "*;q=0, identity").await;

        assert_eq!(response.status(), StatusCode::OK);
        assert!(ran.load(Ordering::SeqCst));
        assert_eq!(response.headers().get(header::CONTENT_ENCODING), None);
    }
}
