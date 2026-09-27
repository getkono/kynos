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
        // Rule 1: absent means everything is acceptable.
        ("no field at all", None, Negotiated::Identity),
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
/// The clamping itself is asserted where it now lives, in
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

/// A response body that fails while `Compression` buffers it.
///
/// In-crate because no public surface builds a body that fails on demand:
/// `Body::from_body` is `pub(crate)`. Nor does one reach this path through
/// `Served`, whose `Accept-Ranges` makes `Compression` leave it alone.
mod failing {
    use std::{
        convert::Infallible,
        io,
        pin::Pin,
        task::{Context, Poll},
    };

    use bytes::Bytes;
    use http_body::{Frame, SizeHint};
    use http_body_util::BodyExt;

    use crate::{
        Router,
        extract::body::text::Text,
        http::{
            Request,
            body::{Body, BoxError},
            header,
        },
        middleware::{Continued, Interceptor, Next, compression::Compression},
        openapi::{Method, PathTemplate},
        router::{endpoint::builder::EndpointBuilder, service::Service},
    };

    /// States 4096 octets, yields 1024 of them, then the connection fails.
    struct Failing(u8);

    impl http_body::Body for Failing {
        type Data = Bytes;
        type Error = BoxError;

        fn poll_frame(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
            self.0 += 1;
            Poll::Ready(match self.0 {
                1 => Some(Ok(Frame::data(Bytes::from(vec![b'a'; 1024])))),
                2 => Some(Err(Box::new(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    "upstream went away",
                )))),
                _ => None,
            })
        }

        fn size_hint(&self) -> SizeHint {
            SizeHint::with_exact(4096)
        }
    }

    /// Replaces whatever the handler produced with a [`Failing`] body.
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
            continued.set_body(Body::from_body(Failing(0)));
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
