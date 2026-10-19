//! The parts of decompression that decide before anything is read, and what it
//! hands on when the read fails.

use std::io;

use async_compression::tokio::bufread::{BrotliEncoder, GzipEncoder, ZstdEncoder};
use bytes::Bytes;

use super::{Coding, Decompression, MAX_CODINGS, Reason, declared, decode, drain_capped};
use crate::{
    Router,
    http::{self, StatusCode, body::Body},
    middleware::testing::{interrupted, read, upload},
    openapi::{Method, PathTemplate},
    router::{endpoint::builder::EndpointBuilder, service::Service},
};

/// Builds a header map carrying `values` as `Content-Encoding`, one field line
/// each -- which is a shape a client may legitimately send, and which a parser
/// reading only the first value would miss.
fn encoded(values: &[&str]) -> http::HeaderMap {
    let mut headers = http::HeaderMap::new();

    for value in values {
        headers.append(
            http::header::CONTENT_ENCODING,
            http::HeaderValue::from_str(value).expect("a usable field value"),
        );
    }

    headers
}

/// The token table, swept rather than sampled. A coding added without a
/// spelling here is a coding this fails on.
#[test]
fn every_coding_is_named_by_the_token_it_is_named_by() {
    const CASES: &[(&str, Option<Coding>)] = &[
        ("zstd", Some(Coding::Zstd)),
        ("br", Some(Coding::Brotli)),
        ("gzip", Some(Coding::Gzip)),
        // RFC 9110 section 8.4.1.3 keeps `x-gzip` as an alias, and clients
        // still send it.
        ("x-gzip", Some(Coding::Gzip)),
        // Section 8.4.1 makes content codings case-insensitive, so a client
        // shouting is a client to be understood rather than refused.
        ("GZIP", Some(Coding::Gzip)),
        ("Br", Some(Coding::Brotli)),
        ("ZStD", Some(Coding::Zstd)),
        // Registered codings this crate does not implement, and one that is
        // not a coding at all. Each must be refused rather than ignored: a body
        // handed on undecoded is a body the handler reads as garbage.
        ("deflate", None),
        ("compress", None),
        ("x-compress", None),
        ("snappy", None),
        ("", None),
    ];

    for (token, expected) in CASES {
        assert_eq!(
            Coding::from_token(token),
            *expected,
            "the token {token:?} was not read as {expected:?}"
        );
    }
}

#[test]
fn a_body_that_names_no_coding_names_no_coding() {
    assert_eq!(declared(&http::HeaderMap::new()), Some(Vec::new()));
}

/// RFC 9110 section 8.4: `identity` SHOULD NOT appear, and a sender that
/// includes it anyway means the body was not encoded. Refusing it would refuse
/// a body that is perfectly readable.
#[test]
fn identity_is_dropped_rather_than_refused() {
    assert_eq!(declared(&encoded(&["identity"])), Some(Vec::new()));
    assert_eq!(
        declared(&encoded(&["identity, gzip"])),
        Some(vec![Coding::Gzip])
    );
}

/// Section 8.4 lists the codings in the order they were applied, so the list is
/// read in that order and undone in reverse.
#[test]
fn a_chain_is_read_in_the_order_it_was_applied() {
    assert_eq!(
        declared(&encoded(&["gzip, br"])),
        Some(vec![Coding::Gzip, Coding::Brotli])
    );
}

/// `Content-Encoding` is a list header, and a list header may arrive split
/// across field lines. Reading only the first would decode half a chain and
/// hand the rest on as garbage.
#[test]
fn a_chain_split_across_field_lines_is_read_whole() {
    assert_eq!(
        declared(&encoded(&["gzip", "br"])),
        Some(vec![Coding::Gzip, Coding::Brotli])
    );
}

#[test]
fn an_unknown_coding_refuses_the_whole_body() {
    assert_eq!(declared(&encoded(&["deflate"])), None);
    assert_eq!(
        declared(&encoded(&["gzip, deflate"])),
        None,
        "a chain is only decodable if every link is"
    );
}

/// Each link costs a decode pass over a body already at the cap, so a long
/// chain is a way to buy work with a small request.
#[test]
fn a_chain_longer_than_the_cap_is_refused() {
    let longest = ["gzip"; MAX_CODINGS].join(", ");
    let overlong = ["gzip"; MAX_CODINGS + 1].join(", ");

    assert!(
        declared(&encoded(&[&longest])).is_some(),
        "the longest permitted chain was refused"
    );
    assert_eq!(declared(&encoded(&[&overlong])), None);
}

/// Unset, the absolute limit is the only bound -- so a body is never refused
/// for expanding, only for being large.
#[test]
fn without_a_ratio_the_absolute_limit_is_the_only_bound() {
    let decompression = Decompression::new(1_000);

    assert_eq!(decompression.bound(1), 1_000);
    assert_eq!(decompression.bound(u64::MAX), 1_000);
}

/// The ratio is the cheaper check and must be the one that binds while it is
/// tighter, or a small body could reach the absolute cap unchallenged.
#[test]
fn the_bound_is_whichever_limit_is_tighter() {
    let decompression = Decompression::new(1_000).max_ratio(10);

    assert_eq!(decompression.bound(1), 10, "the ratio should have bound it");
    assert_eq!(
        decompression.bound(1_000),
        1_000,
        "the absolute limit should have bound it"
    );
    assert_eq!(
        decompression.bound(100),
        1_000,
        "the two coincide here, and the answer is the shared value"
    );
}

/// A body large enough that its ratio bound would overflow must still be bound
/// by the absolute limit rather than wrapping to something small -- or large.
#[test]
fn a_ratio_that_would_overflow_falls_back_to_the_absolute_limit() {
    let decompression = Decompression::new(1_000).max_ratio(u64::MAX);

    assert_eq!(decompression.bound(u64::MAX), 1_000);
}

/// `plain` under `coding`, through the same codec the decoder is built from.
async fn encode(coding: Coding, plain: &[u8]) -> Bytes {
    let input = io::Cursor::new(plain);
    let encoded = match coding {
        Coding::Zstd => drain_capped(ZstdEncoder::new(input), u64::MAX).await,
        // Boxed for the reason `decode` boxes its decoder.
        Coding::Brotli => drain_capped(Box::new(BrotliEncoder::new(input)), u64::MAX).await,
        Coding::Gzip => drain_capped(GzipEncoder::new(input), u64::MAX).await,
    };
    encoded.expect("an encoder over memory does not fail")
}

/// `parts`, each encoded on its own under `coding`, laid end to end.
async fn concatenated(coding: Coding, parts: &[&'static [u8]]) -> Bytes {
    let mut joined = Vec::new();
    for part in parts {
        joined.extend_from_slice(&encode(coding, part).await);
    }
    Bytes::from(joined)
}

/// RFC 1952 section 2.2: a gzip file is a series of members, and RFC 9110
/// section 8.4.1.3 names that format. Stopping at the first member hands the
/// handler a prefix of what the client sent.
#[tokio::test]
async fn every_gzip_member_is_decoded() {
    let body = concatenated(Coding::Gzip, &[b"a", b"b"]).await;

    assert_eq!(
        decode(Coding::Gzip, body, 1024).await,
        Ok(Bytes::from_static(b"ab"))
    );
}

/// RFC 8878 section 3.1: a zstd stream is one or more frames.
#[tokio::test]
async fn every_zstd_frame_is_decoded() {
    let body = concatenated(Coding::Zstd, &[b"a", b"b"]).await;

    assert_eq!(
        decode(Coding::Zstd, body, 1024).await,
        Ok(Bytes::from_static(b"ab"))
    );
}

/// Octets after the coded stream are not part of any coding's format, so a
/// body carrying them did not decode as the coding it declared. Brotli is
/// swept with the rest: RFC 7932 defines one stream, so even a second valid
/// stream is trailing input there.
#[tokio::test]
async fn input_after_the_coded_stream_is_malformed() {
    for coding in [Coding::Zstd, Coding::Brotli, Coding::Gzip] {
        let mut junk = encode(coding, b"a").await.to_vec();
        junk.extend_from_slice(b"junk");

        assert_eq!(
            decode(coding, Bytes::from(junk), 1024).await,
            Err(Reason::Malformed),
            "{coding:?} followed by junk"
        );
    }

    assert_eq!(
        decode(
            Coding::Brotli,
            concatenated(Coding::Brotli, &[b"a", b"b"]).await,
            1024
        )
        .await,
        Err(Reason::Malformed),
        "two brotli streams"
    );
}

/// What a client sent before coding it.
const PLAIN: &[u8] = b"the body the client sent, before any coding was applied";

/// A brotli body decodes to what was coded.
#[tokio::test]
async fn a_brotli_body_decodes_to_what_was_coded() {
    assert_eq!(
        decode(Coding::Brotli, encode(Coding::Brotli, PLAIN).await, 1024).await,
        Ok(Bytes::from_static(PLAIN))
    );
}

/// RFC 9110 section 8.4: `Content-Encoding: gzip, br` says gzip was applied
/// first, so br is undone first. Undoing in the order listed is the control,
/// and it fails: the outer layer is not gzip.
#[tokio::test]
async fn a_mixed_chain_is_undone_in_the_reverse_of_the_order_applied() {
    let applied = encode(Coding::Brotli, &encode(Coding::Gzip, PLAIN).await).await;
    let decompression = Decompression::new(1024);

    assert_eq!(
        decompression
            .undo(&[Coding::Gzip, Coding::Brotli], applied.clone())
            .await,
        Ok(Bytes::from_static(PLAIN))
    );
    assert_eq!(
        decompression
            .undo(&[Coding::Brotli, Coding::Gzip], applied)
            .await,
        Err(Reason::Malformed)
    );
}

/// RFC 9110 section 5.6.1: a recipient ignores empty list elements, so a
/// stray comma names no coding — neither one to refuse nor a layer to undo.
#[test]
fn an_empty_list_element_names_no_coding() {
    let cases: &[(&[&str], &[Coding])] = &[
        (&["gzip, , br"], &[Coding::Gzip, Coding::Brotli]),
        (&[", gzip,"], &[Coding::Gzip]),
        (&[" , "], &[]),
        (&["", "br"], &[Coding::Brotli]),
    ];

    for (lines, expected) in cases {
        assert_eq!(
            declared(&encoded(lines)),
            Some(expected.to_vec()),
            "{lines:?}"
        );
    }
}

/// The request the chain beneath `Decompression` was handed.
mod handed_on {
    use std::{
        convert::Infallible,
        sync::{Arc, Mutex},
    };

    use super::{Coding, PLAIN, encode};
    use crate::{
        Router,
        http::{self, HeaderMap, Request, StatusCode, body::Body},
        middleware::{Continued, Interceptor, Next, decompression::Decompression, testing},
        openapi::{Method, PathTemplate},
        router::{endpoint::builder::EndpointBuilder, service::Service},
    };

    /// Keeps the headers of the request it is handed.
    struct Seen(Arc<Mutex<Option<HeaderMap>>>);

    impl Interceptor<()> for Seen {
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
            *self.0.lock().expect("an unpoisoned lock") = Some(request.headers().clone());
            Ok(next.run(request).await)
        }
    }

    /// The upload under `Decompression`, and what the chain beneath it saw.
    fn service() -> (Service<()>, Arc<Mutex<Option<HeaderMap>>>) {
        let seen = Arc::new(Mutex::new(None));
        let endpoint = EndpointBuilder::new(
            Method::Post,
            PathTemplate::parse("/upload").expect("a valid path"),
            testing::upload,
        );
        let service = Router::<()>::new()
            .mount(endpoint)
            .intercept(Decompression::new(1024))
            .intercept(Seen(Arc::clone(&seen)))
            .build(())
            .expect("a describable router");
        (service, seen)
    }

    /// The three digest fields a client may send, each over the octets it
    /// sent.
    const DIGESTS: [(&str, &str); 3] = [
        ("content-digest", "sha-256=:deadbeef:"),
        ("digest", "sha-256=deadbeef"),
        ("content-md5", "Q2hlY2sgSW50ZWdyaXR5IQ=="),
    ];

    /// `body` posted under `coding` when it names one, carrying every digest
    /// field and its own length.
    fn upload(coding: Option<&str>, body: bytes::Bytes) -> Request {
        let mut builder = ::http::Request::builder()
            .method("POST")
            .uri("/upload")
            .header(http::header::CONTENT_TYPE, "application/octet-stream")
            .header(http::header::CONTENT_LENGTH, body.len());
        if let Some(coding) = coding {
            builder = builder.header(http::header::CONTENT_ENCODING, coding);
        }
        for (name, value) in DIGESTS {
            builder = builder.header(name, value);
        }
        builder
            .body(Body::from_bytes(body))
            .expect("a well-formed request")
    }

    /// Decoding a `gzip, br` chain hands on the octets the client coded,
    /// without the coding, with the decoded length, and with none of
    /// `Content-Digest`, `Digest` or `Content-MD5`: each was computed over
    /// octets no longer there.
    #[tokio::test]
    async fn a_decoded_body_goes_on_without_any_digest_of_the_coded_form() {
        let (service, seen) = service();
        let applied = encode(Coding::Brotli, &encode(Coding::Gzip, PLAIN).await).await;

        let (status, body) =
            testing::read(service.call(upload(Some("gzip, br"), applied)).await).await;
        let seen = seen.lock().expect("an unpoisoned lock").take();
        let seen = seen.expect("the chain beneath ran");

        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        assert_eq!(body, format!("stored {} octets", PLAIN.len()));
        assert_eq!(seen.get(http::header::CONTENT_ENCODING), None);
        assert_eq!(
            seen.get(http::header::CONTENT_LENGTH)
                .map(http::HeaderValue::as_bytes),
            Some(PLAIN.len().to_string().as_bytes())
        );
        for (name, _) in DIGESTS {
            assert_eq!(seen.get(name), None, "{name} survived the decode");
        }
    }

    /// The control: an uncoded body's digests still describe it, and every
    /// one goes on unchanged.
    #[tokio::test]
    async fn an_uncoded_body_keeps_every_digest() {
        let (service, seen) = service();

        let (status, _) = testing::read(
            service
                .call(upload(None, bytes::Bytes::from_static(PLAIN)))
                .await,
        )
        .await;
        let seen = seen.lock().expect("an unpoisoned lock").take();
        let seen = seen.expect("the chain beneath ran");

        assert_eq!(status, StatusCode::OK);
        for (name, value) in DIGESTS {
            assert_eq!(
                seen.get(name).map(http::HeaderValue::as_bytes),
                Some(value.as_bytes()),
                "{name}"
            );
        }
    }
}

/// The upload operation, under `Decompression` when `limit` names one.
fn service(limit: Option<u64>) -> Service<()> {
    let endpoint = EndpointBuilder::new(
        Method::Post,
        PathTemplate::parse("/upload").expect("a valid path"),
        upload,
    );
    decompressed(Router::<()>::new().mount(endpoint), limit)
}

/// `router` built under `Decompression` when `limit` names one.
fn decompressed(router: Router<()>, limit: Option<u64>) -> Service<()> {
    match limit {
        Some(limit) => router.intercept(Decompression::new(limit)).build(()),
        None => router.build(()),
    }
    .expect("a describable router")
}

/// The ratio is measured against what arrived, so stacking codings must not
/// multiply it: `gzip, gzip` over a megabyte of zeroes expands each layer by
/// less than the ratio, and the two together by far more.
#[tokio::test]
async fn a_ratio_bounds_stacked_codings_by_what_arrived() {
    const RATIO: u64 = 1024;
    static ZEROES: [u8; 1 << 20] = [0; 1 << 20];

    let inner = encode(Coding::Gzip, &ZEROES).await;
    let outer = encode(Coding::Gzip, &inner).await;
    let len = |bytes: &[u8]| u64::try_from(bytes.len()).expect("a length fits u64");

    let decompression = Decompression::new(16 << 20).max_ratio(RATIO);
    assert!(
        len(&inner) <= len(&outer) * RATIO && len(&ZEROES) <= len(&inner) * RATIO,
        "each layer alone should expand by less than the ratio"
    );
    assert!(
        len(&ZEROES) > decompression.bound(len(&outer)),
        "the layers together should expand by more than the ratio"
    );

    let endpoint = EndpointBuilder::new(
        Method::Post,
        PathTemplate::parse("/upload").expect("a valid path"),
        upload,
    );
    let service = Router::<()>::new()
        .mount(endpoint)
        .intercept(decompression)
        .build(())
        .expect("a describable router");
    let request = ::http::Request::builder()
        .method("POST")
        .uri("/upload")
        .header(http::header::CONTENT_TYPE, "application/octet-stream")
        .header(http::header::CONTENT_ENCODING, "gzip, gzip")
        .body(Body::from_bytes(outer))
        .expect("a well-formed request");

    let (status, body) = read(service.call(request).await).await;
    assert_eq!(
        status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "{}",
        String::from_utf8_lossy(&body)
    );
}

/// Buffering the body to decode it must not turn a refused request into an
/// accepted one, nor into a different refusal: the extractor beneath answers
/// as it does for an uncoded body that fails with no `Decompression` mounted,
/// a transport failure. Uncoded, because that body reaches the handler with no
/// decode to notice the truncation; and coded, because decoding the part that
/// arrived would answer for a body the client never sent, and leaving its
/// coding declared would blame the coding, which this operation decodes.
#[tokio::test]
async fn a_body_that_fails_part_way_is_refused_as_it_is_without_decompression() {
    let (plain, plain_body) = read(service(None).call(interrupted(None)).await).await;
    assert_eq!(plain, StatusCode::BAD_REQUEST);

    for coding in [None, Some("gzip")] {
        let (decoded, decoded_body) =
            read(service(Some(1024)).call(interrupted(coding)).await).await;

        assert_eq!(
            decoded,
            plain,
            "coding {coding:?}: {}",
            String::from_utf8_lossy(&decoded_body)
        );
        assert_eq!(decoded_body, plain_body, "coding {coding:?}");
    }
}

/// What a streaming extractor beneath `Decompression` reads, which is where
/// handing on only the failure would differ from handing on what arrived with
/// it.
#[cfg(all(feature = "json", feature = "openapi32"))]
mod streamed {
    use super::decompressed;
    use crate::{
        Router,
        http::StatusCode,
        middleware::testing::{interrupted_as, read, tally},
        openapi::{Method, PathTemplate},
        router::{endpoint::builder::EndpointBuilder, service::Service},
    };

    /// The tally operation, under `Decompression` when `limit` names one.
    fn service(limit: Option<u64>) -> Service<()> {
        let endpoint = EndpointBuilder::new(
            Method::Post,
            PathTemplate::parse("/upload").expect("a valid path"),
            tally,
        );
        decompressed(Router::<()>::new().mount(endpoint), limit)
    }

    /// The records that arrived before the failure reach the handler, and the
    /// failure after them: the read fails where it would with no
    /// `Decompression` mounted, not earlier. Uncoded, because that is the body
    /// a streaming reader parses as it arrives.
    #[tokio::test]
    async fn records_before_a_failure_reach_a_streaming_handler_as_without_decompression() {
        let request = || interrupted_as("application/x-ndjson", None, b"1\n2\n");
        let (plain, plain_body) = read(service(None).call(request()).await).await;
        let (decoded, decoded_body) = read(service(Some(1024)).call(request()).await).await;

        assert_eq!(plain, StatusCode::OK);
        assert_eq!(
            plain_body.as_ref(),
            b"read [1, 2], then a rejection",
            "{}",
            String::from_utf8_lossy(&plain_body)
        );
        assert_eq!(decoded, plain);
        assert_eq!(
            decoded_body,
            plain_body,
            "{}",
            String::from_utf8_lossy(&decoded_body)
        );
    }

    /// A coded body that fails part-way hands on only the failure: the coded
    /// octets before it are no records, and its coding, left declared, would be
    /// refused as one nothing decoded.
    #[tokio::test]
    async fn a_coded_body_that_fails_part_way_reaches_a_streaming_handler_as_its_failure() {
        let request = interrupted_as("application/x-ndjson", Some("gzip"), b"1\n2\n");
        let (status, body) = read(service(Some(1024)).call(request).await).await;

        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        assert_eq!(
            body.as_ref(),
            b"read [], then a rejection",
            "{}",
            String::from_utf8_lossy(&body)
        );
    }
}
