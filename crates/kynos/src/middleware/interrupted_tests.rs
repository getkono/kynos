//! The fixtures the body-reading interceptors' tests share.
//!
//! In-crate because no public surface builds a body that fails:
//! `Body::from_body` is `pub(crate)`. One copy, so the limits, decompression
//! and compression tests interrupt a body the same way.

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use http_body::{Frame, SizeHint};
use http_body_util::BodyExt;

#[cfg(all(feature = "json", feature = "openapi32"))]
use crate::extract::body::json_lines::{JsonLines, records::Records};
use crate::{
    extract::body::{binary::Binary, text::Text},
    http::{
        Request, Response, StatusCode,
        body::{Body, BoxError},
        header,
        media::OctetStream,
    },
};

/// One data frame, then the connection fails.
pub(crate) struct Failing {
    /// The octets that arrive before the failure.
    arrived: Bytes,
    /// The exact length the body states, when it states one.
    stated: Option<u64>,
    /// How many times the body has been polled.
    polls: u8,
}

impl Failing {
    /// A body yielding `arrived` and then failing, stating no length.
    pub(crate) fn after(arrived: impl Into<Bytes>) -> Self {
        Self {
            arrived: arrived.into(),
            stated: None,
            polls: 0,
        }
    }

    /// The same body, stating `len` octets it will never finish.
    #[cfg(feature = "compression")]
    pub(crate) fn stating(self, len: u64) -> Self {
        Self {
            stated: Some(len),
            ..self
        }
    }
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
            1 => Some(Ok(Frame::data(self.arrived.clone()))),
            2 => Some(Err(Box::new(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "peer went away",
            )))),
            _ => None,
        })
    }

    fn size_hint(&self) -> SizeHint {
        self.stated
            .map_or_else(SizeHint::default, SizeHint::with_exact)
    }
}

/// Accepts whatever octets arrive, which is what makes a truncation visible:
/// nothing here parses, so nothing here would notice one.
pub(crate) async fn upload(body: Binary<OctetStream>) -> Text {
    Text(format!("stored {} octets", body.into_inner().len()))
}

/// Reads records until the body ends or fails, and says which it did.
#[cfg(all(feature = "json", feature = "openapi32"))]
pub(crate) async fn tally(JsonLines { mut items }: JsonLines<Records<u32>>) -> Text {
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

/// A `POST /upload` of `content_type` carrying `arrived` and then failing,
/// under `coding` when it names one, with no `Content-Length`.
pub(crate) fn interrupted_as(
    content_type: &str,
    coding: Option<&str>,
    arrived: &'static [u8],
) -> Request {
    let mut builder = http::Request::builder()
        .method("POST")
        .uri("/upload")
        .header(header::CONTENT_TYPE, content_type);
    if let Some(coding) = coding {
        builder = builder.header(header::CONTENT_ENCODING, coding);
    }
    builder
        .body(Body::from_body(Failing::after(arrived)))
        .expect("a well-formed request")
}

/// An octet-stream upload that fails part-way, under `coding` when it names
/// one.
pub(crate) fn interrupted(coding: Option<&str>) -> Request {
    interrupted_as("application/octet-stream", coding, b"the first half of")
}

/// A response's status and body bytes.
pub(crate) async fn read(response: Response) -> (StatusCode, Bytes) {
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("a response body that completes")
        .to_bytes();
    (status, body)
}
