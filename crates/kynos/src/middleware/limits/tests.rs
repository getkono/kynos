//! `BodySize` over a request body that fails part-way.
//!
//! In-crate rather than in `tests/limits.rs` because no public surface builds a
//! body that fails: `Body::from_body` is `pub(crate)`, which is the reason
//! `extract/body/json_lines/tests.rs` lives in-crate too. It is ungated, so this
//! module runs at baseline features.

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

/// The upload operation, under `BodySize` when `limit` names one.
fn service(limit: Option<u64>) -> Service<()> {
    let endpoint = EndpointBuilder::new(
        Method::Post,
        PathTemplate::parse("/upload").expect("a valid path"),
        upload,
    );
    let router = Router::<()>::new().mount(endpoint);
    match limit {
        Some(limit) => router.intercept(BodySize::new(limit)).build(()),
        None => router.build(()),
    }
    .expect("a describable router")
}

/// An upload with no `Content-Length`, which is the branch that buffers.
fn interrupted() -> Request {
    http::Request::builder()
        .method("POST")
        .uri("/upload")
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from_body(Failing(0)))
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
