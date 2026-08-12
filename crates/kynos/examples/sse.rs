//! Server-Sent Events, and the two halves of resuming one.
//!
//! Everything here needs OpenAPI 3.2:
//!
//! ```text
//! cargo run -p kynos --example sse --no-default-features \
//!   --features openapi32,macros,server,http1
//! ```
//!
//! [`streaming.rs`](streaming.rs) covers the other sequential media types — the
//! JSON framings and raw byte streams. This file is the one protocol among them
//! with rules of its own.
//!
//! Four things are worth noticing:
//!
//! * **An event names its type twice, and the two must agree.** SSE's `event`
//!   field is what a browser's `addEventListener` selects on; a serde tag inside
//!   the data is what a consumer holding a parsed value branches on. `Update`
//!   below carries the tag and [`Event::event`] carries the name, set from the
//!   same variant — because a stream where they disagree is one no client can
//!   read both ways.
//! * **Resumption is a header and an identifier, and nothing else.** A client
//!   that drops the connection reconnects with `Last-Event-ID` set to the last
//!   [`Event::id`] it saw, and a browser does it without being asked. Minting
//!   ids while ignoring the header is the worse of the two failures: it looks
//!   resumable and replays from the beginning.
//! * **`retry` is advice, and it belongs on the first event.** It sets how long
//!   a client waits before reconnecting, in milliseconds. Repeating it on every
//!   event costs bytes on every event to say what was already said.
//! * **Keep-alive is a comment, so it appears in no description.** The protocol
//!   requires a client to ignore comment lines, which is exactly what makes them
//!   usable to stop an idle connection being reaped by an intermediary. Nothing
//!   about the contract changes, so nothing about the contract records it.
//!
//! 3.1 is not merely inconvenient here: it has no `itemSchema`, so an event
//! stream can only be described as an opaque string. Kynos would rather not
//! compile than emit that.

use std::{
    io,
    net::Ipv4Addr,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use kynos::{
    extract::params::header::Headers,
    prelude::*,
    response::stream::sse::{Event, KeepAlive, Sse},
    server::Server,
};
use serde::{Deserialize, Serialize};

/// One thing that can happen on the feed.
///
/// Internally tagged, so the derive emits a `oneOf` with a `discriminator` on
/// `kind` rather than an `anyOf` a consumer has to guess its way through. An
/// untagged enum is refused outright: serde's first-match tie-break is not
/// expressible in JSON Schema, so there would be nothing truthful to emit.
#[derive(Schema, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Update {
    /// A new measurement.
    Reading {
        /// When it was taken, in milliseconds since the epoch.
        at_millis: i64,
        /// What was measured.
        value: f64,
    },

    /// A threshold was crossed.
    Alert {
        /// Which threshold.
        threshold: String,
    },

    /// The producer is finished and will send nothing further.
    ///
    /// A terminal event rather than a closed connection, because the two are
    /// indistinguishable to a client and only one of them means "do not
    /// reconnect".
    Closed,
}

impl Update {
    /// The SSE event name for this variant.
    ///
    /// The single place the two spellings are tied together. A new variant that
    /// forgets its name is a non-exhaustive match here rather than a stream a
    /// listener silently never fires on.
    fn event_name(&self) -> &'static str {
        match self {
            Self::Reading { .. } => "reading",
            Self::Alert { .. } => "alert",
            Self::Closed => "closed",
        }
    }
}

/// Where a reconnecting client asks to resume from.
///
/// `Last-Event-ID` is sent by a browser automatically on reconnect, and is
/// absent on the first connection — which is why the field is an `Option` and
/// not a required parameter. It is also not one of the three header names a
/// group may not declare, since nothing in the specification says a parameter
/// definition for it is ignored.
#[allow(dead_code)]
#[derive(HeaderParams)]
struct Resume {
    #[header(rename = "Last-Event-ID")]
    last_event_id: Option<String>,
}

/// A feed that can begin part-way through.
///
/// Hand-written, because `futures_core::Stream` is the entire requirement.
/// There is no Kynos stream type to adapt to and no combinator crate in the
/// dependency tree.
struct Feed {
    /// The identifier of the next event, which is what a client sends back.
    next_id: u64,
    /// How many events are left before the feed closes.
    remaining: u32,
    /// Whether the reconnection advice has been sent yet.
    advised: bool,
}

impl Feed {
    /// Starts a feed, resuming after `last_event_id` when one was supplied.
    ///
    /// An identifier this producer never minted is treated as no identifier at
    /// all. A client controls the header, so parsing it is the same act as
    /// validating it.
    fn resuming_after(last_event_id: Option<&str>) -> Self {
        let next_id = last_event_id
            .and_then(|id| id.parse::<u64>().ok())
            .map_or(0, |id| id + 1);

        Self {
            next_id,
            remaining: 10,
            advised: false,
        }
    }
}

impl futures_core::Stream for Feed {
    // A `Result`, because a feed can fail after the status is on the wire. The
    // error terminates the stream; it cannot retract the 200 already sent,
    // which is why a streamed operation should validate everything it can
    // before it returns.
    type Item = Result<Event<Update>, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.remaining == 0 {
            return Poll::Ready(None);
        }

        let id = self.next_id;
        self.next_id += 1;
        self.remaining -= 1;

        let update = if self.remaining == 0 {
            Update::Closed
        } else if id % 5 == 4 {
            Update::Alert {
                threshold: "high".to_owned(),
            }
        } else {
            Update::Reading {
                at_millis: i64::try_from(id).unwrap_or(i64::MAX),
                value: f64::from(u32::try_from(id).unwrap_or(u32::MAX)),
            }
        };

        // Read before the payload moves into the event, so the SSE name and the
        // serde tag come from the same value and cannot drift apart.
        let name = update.event_name();
        let mut event = Event::new(update).event(name).id(id.to_string());

        if !self.advised {
            self.advised = true;
            // Two seconds, sent once. A client that reconnects sooner than the
            // producer can usefully serve it is a client the producer asked for.
            event = event
                .retry(2_000)
                .comment("reconnect after two seconds, resuming from the last id");
        }

        Poll::Ready(Some(Ok(event)))
    }
}

/// Streams updates, resuming where a reconnecting client left off.
///
/// The keep-alive interval is shorter than the idle timeout of the proxies this
/// service expects to sit behind. That is the only thing it has to be shorter
/// than, and picking it is a deployment question rather than a protocol one.
#[kynos::get("/updates")]
async fn updates(Headers(resume): Headers<Resume>) -> Sse<Feed> {
    Sse::new(Feed::resuming_after(resume.last_event_id.as_deref())).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .comment("still connected"),
    )
}

/// Streams a fixed, short burst of updates.
///
/// No keep-alive, because the stream ends long before anything could reap it.
/// Configuring one anyway would send comments nobody is waiting on.
#[kynos::get("/updates/burst")]
async fn burst() -> Sse<Feed> {
    Sse::new(Feed::resuming_after(None))
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new().mount(kynos::routes![updates, burst]);

    // `itemSchema` is what this prints that a 3.1 document could not: the shape
    // of one event, rather than a claim that the body is one JSON value.
    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
