//! `BodySize` over a request body that fails part-way.
//!
//! In-crate rather than in `tests/limits.rs` because no public surface builds a
//! body that fails: `Body::from_body` is `pub(crate)`, which is the reason
//! `extract/body/json_lines/tests.rs` lives in-crate too. It is ungated, so this
//! module runs at baseline features; the one case that reads through a
//! streaming extractor carries that extractor's gate.

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use http_body::Frame;
use http_body_util::BodyExt;

use super::BodySize;
use crate::{
    Router,
    extract::{
        body::{binary::Binary, text::Text},
        media::OctetStream,
    },
    http::{
        Request, Response, StatusCode,
        body::{Body, BoxError},
        header,
    },
    openapi::{Method, PathTemplate},
    router::{endpoint::builder::EndpointBuilder, service::Service},
};

/// One data frame, then the connection fails.
struct Failing {
    /// The octets that arrive before the failure.
    arrived: &'static [u8],
    /// How many times the body has been polled.
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
            1 => Some(Ok(Frame::data(Bytes::from_static(self.arrived)))),
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

/// The upload operation, under `BodySize` when `limit` names one.
fn service(limit: Option<u64>) -> Service<()> {
    let endpoint = EndpointBuilder::new(
        Method::Post,
        PathTemplate::parse("/upload").expect("a valid path"),
        upload,
    );
    limited(Router::<()>::new().mount(endpoint), limit)
}

/// `router` built under `BodySize` when `limit` names one.
fn limited(router: Router<()>, limit: Option<u64>) -> Service<()> {
    match limit {
        Some(limit) => router.intercept(BodySize::new(limit)).build(()),
        None => router.build(()),
    }
    .expect("a describable router")
}

/// A `content_type` upload of `arrived` that then fails, with no
/// `Content-Length`, which is the branch that buffers.
fn interrupted_as(content_type: &str, arrived: &'static [u8]) -> Request {
    http::Request::builder()
        .method("POST")
        .uri("/upload")
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from_body(Failing { arrived, polls: 0 }))
        .expect("a well-formed request")
}

/// An octet-stream upload that fails part-way.
fn interrupted() -> Request {
    interrupted_as("application/octet-stream", b"the first half of")
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

/// Mounting a limit must not turn a refused request into an accepted one: the
/// extractor beneath answers exactly as it does with no limit mounted.
#[tokio::test]
async fn a_body_that_fails_part_way_is_refused_as_it_is_without_the_limit() {
    let (unlimited, unlimited_body) = read(service(None).call(interrupted()).await).await;
    let (limited, limited_body) = read(service(Some(1024)).call(interrupted()).await).await;

    assert_eq!(unlimited, StatusCode::BAD_REQUEST);
    assert_eq!(
        limited,
        unlimited,
        "{}",
        String::from_utf8_lossy(&limited_body)
    );
    assert_eq!(limited_body, unlimited_body);
}

/// What a streaming extractor beneath the limit reads, which is where handing
/// on only the failure would differ from handing on what arrived with it.
#[cfg(all(feature = "json", feature = "openapi32"))]
mod streamed {
    use super::{interrupted_as, limited, read};
    use crate::{
        Router,
        extract::body::{
            json_lines::{JsonLines, records::Records},
            text::Text,
        },
        http::StatusCode,
        openapi::{Method, PathTemplate},
        router::{endpoint::builder::EndpointBuilder, service::Service},
    };

    /// Reads records until the body ends or fails, and says which it did.
    async fn tally(JsonLines { mut items }: JsonLines<Records<u32>>) -> Text {
        let mut read = Vec::new();
        let ending = loop {
            match items.next().await {
                Some(Ok(record)) => read.push(record),
                Some(Err(_)) => break "a rejection",
                None => break "the end of the body",
            }
        };
        Text(format!("read {read:?}, then {ending}"))
    }

    /// The tally operation, under `BodySize` when `limit` names one.
    fn service(limit: Option<u64>) -> Service<()> {
        let endpoint = EndpointBuilder::new(
            Method::Post,
            PathTemplate::parse("/upload").expect("a valid path"),
            tally,
        );
        limited(Router::<()>::new().mount(endpoint), limit)
    }

    /// The records that arrived before the failure reach the handler, and the
    /// failure after them: the read fails where it would with no limit
    /// mounted, not earlier.
    #[tokio::test]
    async fn records_before_a_failure_reach_a_streaming_handler_as_without_the_limit() {
        let request = || interrupted_as("application/x-ndjson", b"1\n2\n");
        let (unlimited, unlimited_body) = read(service(None).call(request()).await).await;
        let (limited, limited_body) = read(service(Some(1024)).call(request()).await).await;

        assert_eq!(unlimited, StatusCode::OK);
        assert_eq!(
            unlimited_body.as_ref(),
            b"read [1, 2], then a rejection",
            "{}",
            String::from_utf8_lossy(&unlimited_body)
        );
        assert_eq!(limited, unlimited);
        assert_eq!(
            limited_body,
            unlimited_body,
            "{}",
            String::from_utf8_lossy(&limited_body)
        );
    }
}
