//! The request body a [`TestRequest`](crate::test::TestRequest) sends, framed
//! as hyper frames one.

use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use http_body::{Body as HttpBody, Frame, SizeHint};

use crate::http::body::BoxError;

/// What a [`TestRequest`](crate::test::TestRequest) sends as its body.
pub(super) enum RequestBody {
    /// Every octet at once; empty once sent.
    Whole(Bytes),
    /// Chunks as a stream yields them.
    Streamed(Pin<Box<dyn futures_core::Stream<Item = Bytes> + Send>>),
}

impl RequestBody {
    /// The next chunk, empty or not.
    pub(super) fn poll_chunk(&mut self, context: &mut Context<'_>) -> Poll<Option<Bytes>> {
        match self {
            Self::Whole(bytes) if bytes.is_empty() => Poll::Ready(None),
            Self::Whole(bytes) => Poll::Ready(Some(std::mem::take(bytes))),
            Self::Streamed(chunks) => chunks.as_mut().poll_next(context),
        }
    }
}

impl fmt::Debug for RequestBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Whole(bytes) => formatter.debug_tuple("Whole").field(bytes).finish(),
            Self::Streamed(_) => formatter.debug_tuple("Streamed").finish_non_exhaustive(),
        }
    }
}

/// A request body read as hyper reads one: frame by frame, and under a
/// declared length exactly that many octets end it, nothing past them is read,
/// and an end before them fails the read.
pub(super) struct Framed {
    pub(super) body: RequestBody,
    /// Octets still owed; `None` when no length was declared.
    pub(super) remaining: Option<u64>,
    /// Set once a short end has been reported.
    pub(super) failed: bool,
}

impl HttpBody for Framed {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        if this.failed || this.remaining == Some(0) {
            return Poll::Ready(None);
        }

        loop {
            let Some(mut chunk) = std::task::ready!(this.body.poll_chunk(context)) else {
                let Some(owed) = this.remaining else {
                    return Poll::Ready(None);
                };
                this.failed = true;
                let short = std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    format!(
                        "the body ended {owed} octet(s) short of its declared `Content-Length`"
                    ),
                );
                return Poll::Ready(Some(Err(short.into())));
            };

            // A server yields no empty data frame.
            if chunk.is_empty() {
                continue;
            }
            if let Some(remaining) = &mut this.remaining {
                chunk.truncate(usize::try_from(*remaining).unwrap_or(usize::MAX));
                *remaining -= chunk.len() as u64;
            }
            return Poll::Ready(Some(Ok(Frame::data(chunk))));
        }
    }

    // Never after a short end, which is a failure rather than an end.
    fn is_end_stream(&self) -> bool {
        self.remaining == Some(0)
    }

    fn size_hint(&self) -> SizeHint {
        self.remaining
            .map_or_else(SizeHint::default, SizeHint::with_exact)
    }
}
