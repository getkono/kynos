//! Responses that are described item by item, and the whole-query-string
//! parameter.
//!
//! Everything here needs OpenAPI 3.2:
//!
//! ```text
//! cargo run -p kynos --example streaming --no-default-features \
//!   --features openapi32,macros,server,http1,json
//! ```
//!
//! Four things are worth noticing:
//!
//! * **3.1 cannot describe a stream, so the whole subtree is 3.2-only.** What
//!   3.2 adds is `itemSchema`: a way to say what one *item* of a sequential
//!   media type looks like, rather than what the whole body looks like. Without
//!   it a streaming response could only be described as bytes, and describing
//!   it as bytes is the thing this framework exists not to do.
//! * **A stream is `futures_core::Stream` and nothing more.** No executor
//!   integration, no combinator crate, no Kynos stream type to adapt to. The
//!   hand-written `Countdown` below is the whole contract, which is what keeps
//!   the dependency at `futures-core` rather than `futures`.
//! * **The status is committed before the body is.** A stream that fails
//!   halfway cannot retract a 200 it already sent, so it terminates. That is a
//!   property of streaming rather than of Kynos, and it is why a streamed
//!   operation should validate everything it can before returning.
//! * **A 206 can hold several parts, and only 3.2 can say so.**
//!   `multipart/byteranges` is a sequential media type whose item *count* the
//!   request decides, so there is no array a `schema` could describe — which is
//!   what `itemSchema` and `itemEncoding` are for, and what the specification's
//!   own *Streaming Byte Ranges* example writes. `RangedParts<T>` is opt-in
//!   rather than something enabling `openapi32` switches on, because a feature
//!   flag that changed what an existing handler put on the wire would not be
//!   the additive thing this one claims to be.
//! * **The same type reads.** `JsonLines<Records<Reading>>` is a request body
//!   rather than a response, and the asymmetry is worth noticing: a request
//!   record that fails still has a status to spend, because nothing reaches
//!   the socket until the handler's future resolves. It is one type in both
//!   directions, so it is imported once, from the module that defines it —
//!   `response::stream::json` re-exports it so a return type reads as a
//!   response, and here it is a return type *and* an argument.
//!
//! Server-Sent Events are the fourth sequential media type, and the only one
//! with protocol rules of its own — event names, resumption, reconnection
//! advice. [`sse.rs`](sse.rs) covers them.
//!
//! `QueryString<T, M>` is the other 3.2-only construct here: a parameter whose
//! value is the *entire* query string, media-typed. It exists for the APIs
//! whose filter language is not `key=value` pairs — a JSON filter, an RSQL
//! expression — which 3.1 could describe only by lying about the shape.

use std::{
    net::Ipv4Addr,
    pin::Pin,
    task::{Context, Poll},
};

use kynos::{
    error::rejection::{BodyRejection, RangeRejection},
    extract::{
        body::{
            binary::Binary,
            json_lines::{JsonLines, JsonSeq, records::Records},
        },
        media::{MediaType, OctetStream},
        params::query::QueryString,
    },
    prelude::*,
    response::{
        range::{Range, parts::RangedParts},
        status::NoContent,
        stream::binary::BinaryStream,
    },
    server::Server,
};
use serde::{Deserialize, Serialize};

/// One reading in a sequence.
#[derive(Schema, Serialize, Deserialize)]
struct Reading {
    at_millis: i64,
    value: f64,
}

/// A finite stream, written by hand.
///
/// `futures_core::Stream` is the entire requirement, so this file needs no
/// stream library at all — which is the point of the demonstration as much as
/// the streaming is.
struct Countdown {
    remaining: u32,
}

impl futures_core::Stream for Countdown {
    type Item = Reading;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.remaining == 0 {
            return Poll::Ready(None);
        }
        self.remaining -= 1;
        Poll::Ready(Some(Reading {
            at_millis: i64::from(self.remaining),
            value: f64::from(self.remaining),
        }))
    }
}

/// The same, producing raw chunks.
struct Chunks {
    remaining: u32,
}

impl futures_core::Stream for Chunks {
    type Item = Result<bytes::Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.remaining == 0 {
            return Poll::Ready(None);
        }
        self.remaining -= 1;
        Poll::Ready(Some(Ok(bytes::Bytes::from_static(b"chunk"))))
    }
}

/// The media type an export is served under.
struct Csv;

impl MediaType for Csv {
    const MEDIA_TYPE: &'static str = "text/csv";
}

/// The filter language this API accepts in a query string.
///
/// Media-typed, because the whole query string is one value in a format —
/// which is exactly what a `key=value` parameter list cannot describe.
struct RsqlFilter;

impl MediaType for RsqlFilter {
    const MEDIA_TYPE: &'static str = "application/rsql";
}

/// Streams readings as newline-delimited JSON.
///
/// One JSON value per line. `itemSchema` describes the `Reading`, so a consumer
/// knows what one line holds without the description claiming the body is one.
#[kynos::get("/readings.jsonl")]
async fn stream_lines() -> JsonLines<Countdown> {
    JsonLines {
        items: Countdown { remaining: 10 },
    }
}

/// Streams readings as an RFC 7464 JSON text sequence.
///
/// The same items under a different framing — record-separator delimited rather
/// than newline — which matters when a value can itself contain a newline.
#[kynos::get("/readings.json-seq")]
async fn stream_sequence() -> JsonSeq<Countdown> {
    JsonSeq {
        items: Countdown { remaining: 10 },
    }
}

/// Streams an export as raw bytes.
///
/// The marker names the media type, exactly as it does for a non-streamed
/// `Binary<M>` body.
#[kynos::get("/readings/export")]
async fn stream_export() -> BinaryStream<Chunks, Csv> {
    BinaryStream::new(Chunks { remaining: 10 })
}

/// Searches readings with a whole-query-string filter.
///
/// `#[kynos::query]` declares the HTTP `QUERY` method, which OpenAPI 3.2 added
/// a Path Item field for — a search with a body, so a long filter is not a URL
/// of unbounded length. It pairs naturally with a media-typed query string, but
/// the two are independent.
#[kynos::query("/readings")]
async fn search(filter: QueryString<String, RsqlFilter>) -> JsonLines<Countdown> {
    let _ = filter.into_inner();
    JsonLines {
        items: Countdown { remaining: 10 },
    }
}

/// Serves a recording, whole or in as many parts as were asked for.
///
/// ```text
/// curl -r 0-3,8-11 http://localhost:3000/recording -D -
/// ```
///
/// Three statuses, none chosen at run time: a 200 for a field Kynos cannot
/// apply, a 206 for one it can, and a 416 for one nothing satisfies. The 206
/// declares two shapes, because the request decides which arrives — one part
/// after coalescing is the recording's own media type, and several is
/// `multipart/byteranges` with a `Content-Range` inside each part rather than
/// on the response.
///
/// Overlapping and adjacent parts are merged before anything is written, which
/// is what makes `bytes=0-0,0-0,0-0,...` cost one octet rather than three.
#[kynos::get("/recording")]
async fn recording(
    range: Range<Binary<OctetStream>>,
) -> Result<RangedParts<Binary<OctetStream>>, RangeRejection> {
    range.apply_parts(Binary::new(&b"0123456789abcdef"[..]))
}

/// Ingests readings as newline-delimited JSON, one record at a time.
///
/// The reading half of what `/readings.jsonl` writes, and the request body
/// never exists in memory as a whole. A record that is not JSON ends the
/// stream with a 400; one that is JSON and does not fit `Reading` is a 422
/// naming which record it was, and the record after it still arrives. Both
/// statuses are already on the operation, because they are the ones
/// `BodyRejection` declares.
#[kynos::post("/readings")]
async fn ingest(
    JsonLines { mut items }: JsonLines<Records<Reading>>,
) -> Result<NoContent, BodyRejection> {
    let mut total = 0.0;
    while let Some(reading) = items.next().await {
        total += reading?.value;
    }

    println!("ingested readings totalling {total}");
    Ok(NoContent)
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new().mount(kynos::routes![
        stream_lines,
        stream_sequence,
        stream_export,
        search,
        ingest,
        recording,
    ]);

    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
