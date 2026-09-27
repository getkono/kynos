//! The parts of decompression that decide before anything is read, and what it
//! hands on when the read fails.

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use http_body::Frame;
use http_body_util::BodyExt;

use super::{Coding, Decompression, MAX_CODINGS, declared};
use crate::{
    Router,
    extract::{
        body::{binary::Binary, text::Text},
        media::OctetStream,
    },
    http::{
        self, Request, Response, StatusCode,
        body::{Body, BoxError},
    },
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

/// One data frame, then the connection fails.
///
/// Hand-written through the `pub(crate)` `Body::from_body` for the reason
/// `limits/tests.rs` writes its own: no public surface builds a body that
/// fails.
struct Failing {
    polls: u8,
}

impl http_body::Body for Failing {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        self.polls += 1;
        Poll::Ready(match self.polls {
            1 => Some(Ok(Frame::data(Bytes::from_static(b"the first half of")))),
            2 => Some(Err(Box::new(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "peer went away",
            )))),
            _ => None,
        })
    }
}

/// Accepts whatever octets arrive, which is what makes a truncation visible:
/// nothing here parses, so nothing here would notice one.
async fn upload(body: Binary<OctetStream>) -> Text {
    Text(format!("stored {} octets", body.into_inner().len()))
}

/// The upload operation, under `Decompression` when `limit` names one.
fn service(limit: Option<u64>) -> Service<()> {
    let endpoint = EndpointBuilder::new(
        Method::Post,
        PathTemplate::parse("/upload").expect("a valid path"),
        upload,
    );
    let router = Router::<()>::new().mount(endpoint);
    match limit {
        Some(limit) => router.intercept(Decompression::new(limit)).build(()),
        None => router.build(()),
    }
    .expect("a describable router")
}

/// An upload that fails part-way, under `coding` when it names one.
fn interrupted(coding: Option<&str>) -> Request {
    let mut builder = ::http::Request::builder()
        .method("POST")
        .uri("/upload")
        .header(http::header::CONTENT_TYPE, "application/octet-stream");
    if let Some(coding) = coding {
        builder = builder.header(http::header::CONTENT_ENCODING, coding);
    }
    builder
        .body(Body::from_body(Failing { polls: 0 }))
        .expect("a well-formed request")
}

/// A response's status and body bytes.
async fn read(response: Response) -> (StatusCode, Bytes) {
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("a response body that completes")
        .to_bytes();
    (status, body)
}

/// Buffering the body to decode it must not turn a refused request into an
/// accepted one, nor into a different refusal: the extractor beneath answers
/// exactly as it does with no `Decompression` mounted. Uncoded, because that
/// body reaches the handler with no decode to notice the truncation; and coded,
/// because decoding the part that arrived would answer for a body the client
/// never sent.
#[tokio::test]
async fn a_body_that_fails_part_way_is_refused_as_it_is_without_decompression() {
    for coding in [None, Some("gzip")] {
        let (plain, plain_body) = read(service(None).call(interrupted(coding)).await).await;
        let (decoded, decoded_body) =
            read(service(Some(1024)).call(interrupted(coding)).await).await;

        assert_eq!(plain, StatusCode::BAD_REQUEST, "coding {coding:?}");
        assert_eq!(
            decoded,
            plain,
            "coding {coding:?}: {}",
            String::from_utf8_lossy(&decoded_body)
        );
        assert_eq!(decoded_body, plain_body, "coding {coding:?}");
    }
}
