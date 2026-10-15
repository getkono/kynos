//! The stream group's handlers: sixteen items from an iterator, framed by
//! each streaming response.
//!
//! The items are ready as soon as they are polled, so a stream is drained on
//! the poll that produces its head, and what is counted is the framing and
//! not a wait.

use std::convert::Infallible;
use std::iter::Map;
use std::ops::Range;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_core::Stream;
use kynos::{
    extract::body::json_lines::{JsonLines, JsonSeq},
    http::media::OctetStream,
    prelude::*,
    response::stream::{
        binary::BinaryStream,
        sse::{Event, KeepAlive, Sse},
    },
};

use super::{App, Small, small};

/// How many items every stream row produces.
pub(crate) const ITEMS: usize = 16;

/// A stream over an iterator, each item ready on the poll that asks for it.
pub(crate) struct Iter<I>(I);

impl<I: Iterator + Unpin> Stream for Iter<I> {
    type Item = I::Item;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<I::Item>> {
        let _ = context;
        Poll::Ready(self.0.next())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

/// [`ITEMS`] items, each made by calling one function; named rather than
/// `impl Iterator` so that a handler's return type can be.
type Items<T> = Iter<Map<Range<usize>, fn(usize) -> T>>;

/// [`ITEMS`] items made by `item`.
fn items<T>(item: fn(usize) -> T) -> Items<T> {
    Iter((0..ITEMS).map(item))
}

/// `json-small`'s document.
fn document(_: usize) -> Small {
    small()
}

/// `json-small`'s document, as an event: a `Result`, the item an event
/// stream takes.
#[expect(clippy::unnecessary_wraps)]
fn event(_: usize) -> Result<Event<Small>, Infallible> {
    Ok(Event::new(small()))
}

/// `plaintext`'s octets, as a chunk: a `Result`, the item a binary stream
/// takes.
#[expect(clippy::unnecessary_wraps)]
fn chunk(_: usize) -> Result<Bytes, Infallible> {
    Ok(Bytes::from_static(b"Hello, World!"))
}

#[kynos::get("/stream/sse")]
async fn sse() -> Sse<Items<Result<Event<Small>, Infallible>>> {
    Sse::new(items(event))
}

/// [`sse`] with keep-alive comments armed: none is written, since every
/// event is ready before the first interval passes, so what is counted is the
/// timer.
#[kynos::get("/stream/sse/keep-alive")]
async fn sse_keep_alive() -> Sse<Items<Result<Event<Small>, Infallible>>> {
    Sse::new(items(event)).keep_alive(KeepAlive::new())
}

#[kynos::get("/stream/ndjson")]
async fn ndjson() -> JsonLines<Items<Small>> {
    JsonLines {
        items: items(document),
    }
}

#[kynos::get("/stream/json-seq")]
async fn json_seq() -> JsonSeq<Items<Small>> {
    JsonSeq {
        items: items(document),
    }
}

#[kynos::get("/stream/binary")]
async fn binary_stream() -> BinaryStream<Items<Result<Bytes, Infallible>>, OctetStream> {
    BinaryStream::new(items(chunk))
}

/// Mounts this group's handlers.
pub(crate) fn mount(router: Router<App>) -> Router<App> {
    router.mount(kynos::routes![
        sse,
        sse_keep_alive,
        ndjson,
        json_seq,
        binary_stream
    ])
}
