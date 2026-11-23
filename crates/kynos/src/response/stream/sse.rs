//! Server-Sent Events.

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use kynos_openapi::{
    SchemaObject,
    model::{
        body::mime_names,
        schema::types::{SchemaType, TypeSet},
    },
};

#[cfg(feature = "server")]
use crate::server::lifecycle::Draining;
use crate::{
    http::{
        HeaderValue, Response,
        body::{Body, BoxError},
    },
    response::{
        IntoResponse, Responses,
        codec::{ok, with_content_type},
        stream::records::RecordBuffer,
    },
    schema::{Schema, registry::Registry},
};

/// A Server-Sent Events response.
///
/// ```no_run
/// use std::time::Duration;
/// use kynos::response::stream::sse::{KeepAlive, Sse};
///
/// fn events<S>(stream: S) -> Sse<S> {
///     Sse::new(stream).keep_alive(
///         KeepAlive::new()
///             .interval(Duration::from_secs(10))
///             .comment("still connected"),
///     )
/// }
/// ```
///
/// `S` is a [`Stream`](super::Stream) of [`SseItem`]s, and that re-export is
/// how a handler names it without a `futures-core` dependency of its own. A
/// stream that cannot fail yields each [`Event`] as it is:
///
/// ```no_run
/// use kynos::response::stream::{
///     Stream,
///     sse::{Event, Sse},
/// };
///
/// fn events(
///     ticks: impl Stream<Item = Event<u32>> + Send + 'static,
/// ) -> Sse<impl Stream<Item = Event<u32>> + Send + 'static> {
///     Sse::new(ticks)
/// }
/// # fn answers(
/// #     ticks: impl Stream<Item = Event<u32>> + Send + 'static,
/// # ) -> impl kynos::response::IntoResponse + kynos::response::Responses {
/// #     events(ticks)
/// # }
/// ```
///
/// One that can yields `Result<Event<T>, E>`, and its first `Err` ends the
/// body there, since the status was sent with the head:
///
/// ```no_run
/// use std::io;
/// use kynos::response::stream::{
///     Stream,
///     sse::{Event, Sse},
/// };
///
/// fn events(
///     reads: impl Stream<Item = Result<Event<u32>, io::Error>> + Send + 'static,
/// ) -> Sse<impl Stream<Item = Result<Event<u32>, io::Error>> + Send + 'static> {
///     Sse::new(reads)
/// }
/// # fn answers(
/// #     reads: impl Stream<Item = Result<Event<u32>, io::Error>> + Send + 'static,
/// # ) -> impl kynos::response::IntoResponse + kynos::response::Responses {
/// #     events(reads)
/// # }
/// ```
///
/// # Disconnect and backpressure
///
/// Both are the stream's own, and neither needs a callback.
///
/// A client that goes away drops the response body, and the body owns the
/// stream — so `S` is dropped, and whatever it holds is released. Put the
/// cleanup in `S`'s [`Drop`] and it runs when the reader leaves. An
/// [`Observer`](crate::middleware::Observer) hears the same event as
/// [`on_disconnect`](crate::middleware::Observer::on_disconnect), which is where
/// a metric for abandoned streams belongs. It is called after `S` is dropped,
/// so by the time an observer hears of the departure, `S`'s cleanup has run.
///
/// Backpressure is the absence of a poll. Events are pulled from `S` as the
/// connection takes them, one at a time and never ahead, so a slow reader
/// simply leaves `S` unpolled rather than filling a queue behind it. Nothing
/// here buffers on the stream's behalf.
///
/// [`tests/sse.rs`](https://github.com/getkono/kynos/blob/master/crates/kynos/tests/sse.rs)
/// asserts the disconnect over a real socket; backpressure has no test of its
/// own.
#[derive(Debug)]
pub struct Sse<S> {
    /// The stream of events.
    pub events: S,
    keep_alive: Option<KeepAlive>,
    #[cfg(feature = "server")]
    draining: Draining,
}

impl<S> Sse<S> {
    /// Creates an event stream without keep-alive messages.
    #[must_use]
    pub fn new(events: S) -> Self {
        Self {
            events,
            keep_alive: None,
            #[cfg(feature = "server")]
            draining: Draining::never(),
        }
    }

    /// Configures periodic keep-alive comments.
    #[must_use]
    pub fn keep_alive(mut self, keep_alive: KeepAlive) -> Self {
        self.keep_alive = Some(keep_alive);
        self
    }

    /// Ends the stream cleanly once the server serving the request begins its
    /// graceful shutdown, and at once where it already has.
    ///
    /// A drain lets a response already streaming finish rather than ending
    /// it, so an event stream left open holds the drain until the shutdown
    /// timeout. Ended this way, the body stops between two events, its last
    /// one whole: an `EventSource` reconnects, with the `Last-Event-ID` of
    /// the last event it read, to whichever instance serves next. `events`
    /// is dropped with the body, as it is when the client leaves.
    ///
    /// `draining` is the handler's own argument. A request no server listener
    /// accepted reads one that never resolves, so the stream then ends only
    /// as it would have without this.
    ///
    /// ```no_run
    /// use kynos::{response::stream::sse::Sse, server::lifecycle::Draining};
    ///
    /// fn events<S>(draining: Draining, stream: S) -> Sse<S> {
    ///     Sse::new(stream).end_on_shutdown(draining)
    /// }
    /// ```
    #[cfg(feature = "server")]
    #[must_use]
    pub fn end_on_shutdown(mut self, draining: Draining) -> Self {
        self.draining = draining;
        self
    }
}

/// Keep-alive configuration for a Server-Sent Events stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeepAlive {
    interval: std::time::Duration,
    comment: String,
}

impl KeepAlive {
    /// Creates the default keep-alive configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            interval: std::time::Duration::from_secs(15),
            comment: String::new(),
        }
    }

    /// Sets the interval between keep-alive messages.
    ///
    /// # Panics
    ///
    /// Panics if `interval` is zero. A zero interval would re-arm the timer at
    /// the instant it fired, writing keep-alive messages back to back for as
    /// long as the stream stays idle.
    #[must_use]
    pub fn interval(mut self, interval: std::time::Duration) -> Self {
        assert!(
            !interval.is_zero(),
            "a keep-alive interval must be greater than zero"
        );
        self.interval = interval;
        self
    }

    /// Sets the comment carried by each keep-alive message.
    #[must_use]
    pub fn comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = comment.into();
        self
    }
}

impl Default for KeepAlive {
    fn default() -> Self {
        Self::new()
    }
}

/// One Server-Sent Event.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Event<T> {
    /// The event payload, written as JSON.
    ///
    /// `itemSchema` describes the parsed event rather than this value alone, as
    /// OpenAPI 3.2 requires for `text/event-stream`; the payload is reached
    /// through the `data` field's `contentMediaType` and `contentSchema`.
    pub data: T,
    /// The event name, matched by a client's listener.
    pub event: Option<String>,
    /// The event id, which a client returns as `Last-Event-ID` on reconnect.
    pub id: Option<String>,
    /// How long a client should wait before reconnecting.
    ///
    /// The format carries whole milliseconds, so any sub-millisecond remainder
    /// is rounded up on the wire.
    pub retry: Option<std::time::Duration>,
    /// A comment sent before the event data, if any.
    pub comment: Option<String>,
}

impl<T> Event<T> {
    /// Creates an event carrying typed data.
    #[must_use]
    pub fn new(data: T) -> Self {
        Self {
            data,
            event: None,
            id: None,
            retry: None,
            comment: None,
        }
    }

    /// Sets the event name.
    #[must_use]
    pub fn event(mut self, event: impl Into<String>) -> Self {
        self.event = Some(event.into());
        self
    }

    /// Sets the event identifier.
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets how long a client should wait before reconnecting.
    ///
    /// The format carries whole milliseconds, so any sub-millisecond remainder
    /// is rounded up on the wire.
    #[must_use]
    pub fn retry(mut self, retry: std::time::Duration) -> Self {
        self.retry = Some(retry);
        self
    }

    /// Adds an SSE comment to this event.
    #[must_use]
    pub fn comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = Some(comment.into());
        self
    }
}

impl<T: serde::Serialize + Schema> Event<T> {
    /// Creates an event named by its payload's serde tag.
    ///
    /// The name is the value serde writes under `#[serde(tag = "...")]`, the
    /// renames applied, so the tag is spelled once and the event's name
    /// cannot drift from it. The payload is an internally or adjacently
    /// tagged enum, or a struct carrying its own `tag`.
    ///
    /// ```
    /// use kynos::{Schema, response::stream::sse::Event};
    /// use serde::Serialize;
    ///
    /// #[derive(Schema, Serialize)]
    /// #[serde(tag = "kind", rename_all = "snake_case")]
    /// enum ServerEvent {
    ///     Tick { fingerprint: String },
    ///     ShuttingDown,
    /// }
    ///
    /// let event = Event::tagged(ServerEvent::Tick {
    ///     fingerprint: "0123456789abcdef".to_owned(),
    /// });
    /// assert_eq!(event.event.as_deref(), Some("tick"));
    ///
    /// let event = Event::tagged(ServerEvent::ShuttingDown);
    /// assert_eq!(event.event.as_deref(), Some("shutting_down"));
    /// # // The other two shapes the derive describes a tag for.
    /// # #[derive(Schema, Serialize)]
    /// # #[serde(tag = "t", content = "c")]
    /// # enum Adjacent { Moved(u8) }
    /// # assert_eq!(Event::tagged(Adjacent::Moved(1)).event.as_deref(), Some("Moved"));
    /// # #[derive(Schema, Serialize)]
    /// # #[serde(tag = "type", rename = "ping")]
    /// # struct Ping { at: u64 }
    /// # assert_eq!(Event::tagged(Ping { at: 1 }).event.as_deref(), Some("ping"));
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if `data` does not begin with a string member, as anything
    /// serde does not write as an object, an externally tagged enum
    /// included, does not. Serde writes a tag first, so this is caught on
    /// the first event of a misused type.
    ///
    /// A struct whose first field is a string is indistinguishable from a
    /// tag on the wire, so with debug assertions on this also panics where
    /// `T`'s [`Schema`] does not name that member as its tag: as an enum's
    /// `discriminator`, or as a struct's tag property's `const`.
    #[must_use]
    pub fn tagged(data: T) -> Self {
        let Some((member, name)) = leading_member(&data) else {
            panic!(
                "`Event::tagged` names an event by its payload's serde tag, and this payload \
                 writes none: tag its type with `#[serde(tag = \"...\")]`, or name the event \
                 with `Event::new(data).event(name)`"
            );
        };
        debug_assert!(
            describes_tag::<T>(&member, &name),
            "`Event::tagged` read `{member}` as this payload's serde tag, but its schema does \
             not describe a tag there: tag its type with `#[serde(tag = \"...\")]`, or name \
             the event with `Event::new(data).event(name)`"
        );

        Self::new(data).event(name)
    }
}

/// Whether `T`'s own schema names `member` as the tag whose value is `name`.
///
/// An enum's names it as its `discriminator`; a struct carrying a `tag`, which
/// has no branches to discriminate, as a property whose `const` is `name`.
fn describes_tag<T: Schema>(member: &str, name: &str) -> bool {
    let kynos_openapi::Schema::Object(schema) = T::schema(&mut Registry::new()) else {
        return false;
    };

    schema
        .discriminator
        .as_ref()
        .is_some_and(|discriminator| discriminator.property_name == member)
        || schema.properties.get(member).is_some_and(|property| {
            property.as_object().is_some_and(|property| {
                property
                    .const_value
                    .as_ref()
                    .is_some_and(|value| value.as_str() == Some(name))
            })
        })
}

/// The first member `data` serializes, as its key and its string value.
///
/// Serde writes an internal or adjacent tag as the first member of the
/// object, so this is the tag where there is one. Serialization stops at that
/// member, which costs no more than the tag itself whatever the payload
/// holds. `None` when `data` is not an object, or its first member is not a
/// string.
fn leading_member<T: serde::Serialize + ?Sized>(data: &T) -> Option<(String, String)> {
    let mut found = None;
    // Every path ends in `Stop`, the one that found the member included.
    let _ = data.serialize(leading::Leading { found: &mut found });
    found
}

/// What an [`Sse`] stream yields: an [`Event`], or a `Result` of one.
///
/// A bare event is a stream that cannot fail, so it needs no `Ok` around each
/// item and no `Infallible` in its type. An `Err` ends the body after the
/// last whole event, and is reported to the connection as the body's error.
///
/// Sealed: these two are the whole set.
pub trait SseItem: sealed::Sealed {
    /// The payload each event carries.
    type Data;

    /// The error that ends the stream; [`Infallible`](std::convert::Infallible)
    /// for a bare event.
    type Error: Into<BoxError>;

    /// The event this item carries, or the error that ends the stream.
    ///
    /// # Errors
    ///
    /// The item's own error, which a bare event never has.
    fn into_event(self) -> Result<Event<Self::Data>, Self::Error>;
}

/// Closes [`SseItem`], so a third item shape cannot change what the two
/// existing ones mean.
mod sealed {
    /// The private supertrait.
    pub trait Sealed {}
}

impl<T> sealed::Sealed for Event<T> {}
impl<T, E> sealed::Sealed for Result<Event<T>, E> {}

impl<T> SseItem for Event<T> {
    type Data = T;
    type Error = std::convert::Infallible;

    fn into_event(self) -> Result<Event<T>, Self::Error> {
        Ok(self)
    }
}

impl<T, E: Into<BoxError>> SseItem for Result<Event<T>, E> {
    type Data = T;
    type Error = E;

    fn into_event(self) -> Self {
        self
    }
}

/// The events, framed as the `text/event-stream` bytes that carry them.
///
/// The stream is held boxed so that it can be polled without a projection:
/// `Pin<Box<S>>` is `Unpin` whatever `S` is, and `unsafe` is forbidden here.
struct Records<S> {
    events: Pin<Box<S>>,
    keep_alive: Option<Heartbeat>,
    buffer: RecordBuffer,
    /// Each event's data as JSON, before [`field`] frames it; kept so that
    /// it grows once per stream rather than once per event.
    data: Vec<u8>,
    /// Resolves once the server begins draining; `None` when nothing ends
    /// the stream early, or once it has.
    #[cfg(feature = "server")]
    shutdown: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
    /// Set once the shutdown ended the stream, so it stays ended.
    #[cfg(feature = "server")]
    ended: bool,
}

/// The timer that keeps an idle stream from being reaped.
///
/// Only the stream knows when it last produced, so the timer lives here; a
/// runtime allowance in `docs/architecture.md`.
struct Heartbeat {
    /// How long a silence may last before a comment is sent.
    interval: std::time::Duration,
    /// Boxed so it is `Unpin`, as [`Records::events`] is.
    sleep: Pin<Box<tokio::time::Sleep>>,
    /// The comment record, rendered once.
    record: bytes::Bytes,
}

impl Heartbeat {
    fn new(keep_alive: &KeepAlive) -> Self {
        Self {
            interval: keep_alive.interval,
            sleep: Box::pin(tokio::time::sleep(keep_alive.interval)),
            record: heartbeat_record(&keep_alive.comment),
        }
    }

    /// Pushes the deadline out by one interval.
    fn restart(&mut self) {
        self.sleep
            .as_mut()
            .reset(tokio::time::Instant::now() + self.interval);
    }
}

/// The bytes one keep-alive message occupies on the wire.
///
/// A comment record ended by a blank line; an empty comment gives the bare `: `
/// heartbeat every SSE client ignores.
fn heartbeat_record(comment: &str) -> bytes::Bytes {
    let mut record = bytes::BytesMut::new();
    field(&mut record, "", comment.as_bytes());
    record.extend_from_slice(b"\n");
    record.freeze()
}

impl<S> futures_core::Stream for Records<S>
where
    S: futures_core::Stream<Item: SseItem<Data: serde::Serialize>>,
{
    type Item = Result<bytes::Bytes, BoxError>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let records = self.get_mut();

        // Before the events, so a stream busy enough never to be pending
        // still ends; between two records, so the last one sent is whole.
        #[cfg(feature = "server")]
        {
            if records.ended {
                return Poll::Ready(None);
            }
            let began = records
                .shutdown
                .as_mut()
                .is_some_and(|shutdown| shutdown.as_mut().poll(context).is_ready());
            if began {
                records.shutdown = None;
                records.ended = true;
                return Poll::Ready(None);
            }
        }

        // Events first: each resets the silence, so a busy stream sends no comment.
        match records.events.as_mut().poll_next(context) {
            Poll::Ready(Some(item)) => {
                let event = match item.into_event() {
                    Ok(event) => event,
                    Err(error) => return Poll::Ready(Some(Err(error.into()))),
                };
                if let Some(keep_alive) = records.keep_alive.as_mut() {
                    keep_alive.restart();
                }
                let data = &mut records.data;
                return Poll::Ready(Some(
                    records.buffer.record(|record| encode(record, data, &event)),
                ));
            }
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Pending => {}
        }

        let Some(keep_alive) = records.keep_alive.as_mut() else {
            return Poll::Pending;
        };

        // Both futures now hold the waker, so whichever fires first wakes the task.
        match keep_alive.sleep.as_mut().poll(context) {
            Poll::Ready(()) => {
                keep_alive.restart();
                Poll::Ready(Some(Ok(keep_alive.record.clone())))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Writes one event into `record` as a `text/event-stream` record, its data
/// serialized as JSON through `data` first.
///
/// The data is framed by [`field`] like every other value rather than written
/// straight into `record`: compact JSON escapes every line break it writes, but
/// a `serde_json` built with `raw_value` copies a raw value through verbatim.
fn encode<T: serde::Serialize>(
    record: &mut bytes::BytesMut,
    data: &mut Vec<u8>,
    event: &Event<T>,
) -> Result<(), BoxError> {
    data.clear();
    serde_json::to_writer(&mut *data, &event.data)?;

    if let Some(comment) = &event.comment {
        field(record, "", comment.as_bytes());
    }
    if let Some(name) = &event.event {
        field(record, "event", name.as_bytes());
    }
    if let Some(id) = &event.id {
        field(record, "id", id.as_bytes());
    }
    if let Some(retry) = event.retry {
        field(record, "retry", milliseconds(retry).to_string().as_bytes());
    }
    field(record, "data", data);
    record.extend_from_slice(b"\n");

    Ok(())
}

/// A reconnect delay as the whole milliseconds `retry` carries.
///
/// Rounded up, since truncating a sub-millisecond delay to `0` would mean
/// "reconnect at once". An explicit zero stays zero.
fn milliseconds(retry: std::time::Duration) -> u128 {
    let whole = retry.as_millis();
    if retry.subsec_nanos() % 1_000_000 > 0 {
        whole + 1
    } else {
        whole
    }
}

/// Writes one field, one line per line of its value.
///
/// This is how the format carries a newline. A line ends at CR, LF or CRLF,
/// as the format's own `end-of-line` does, so no break in `value` reaches
/// the wire to start a field a client would read on its own. An empty
/// `name` writes the comment form, `: text`.
///
/// Bytes rather than a `str`, so serialized JSON is framed without being
/// re-validated: CR and LF are ASCII, and never part of a longer UTF-8 sequence.
fn field(record: &mut bytes::BytesMut, name: &str, value: &[u8]) {
    let mut push = |line: &[u8]| {
        record.extend_from_slice(name.as_bytes());
        record.extend_from_slice(b": ");
        record.extend_from_slice(line);
        record.extend_from_slice(b"\n");
    };

    let mut rest = value;
    while let Some(end) = rest.iter().position(|&byte| byte == b'\r' || byte == b'\n') {
        push(&rest[..end]);
        // A CRLF is one break; a CR anywhere else, the last in the value
        // included, is a break of its own.
        let width = if rest[end..].starts_with(b"\r\n") {
            2
        } else {
            1
        };
        rest = &rest[end + width..];
    }
    push(rest);
}

impl<S> IntoResponse for Sse<S>
where
    S: futures_core::Stream<Item: SseItem<Data: serde::Serialize>> + Send + 'static,
{
    fn into_response(self) -> Response {
        let records = Records {
            events: Box::pin(self.events),
            keep_alive: self.keep_alive.as_ref().map(Heartbeat::new),
            buffer: RecordBuffer::new(),
            data: Vec::new(),
            #[cfg(feature = "server")]
            shutdown: self.draining.into_signal(),
            #[cfg(feature = "server")]
            ended: false,
        };

        with_content_type(
            Body::from_stream(records),
            HeaderValue::from_static(mime_names::TEXT_EVENT_STREAM),
        )
    }
}

impl<S> Responses for Sse<S>
where
    S: futures_core::Stream<Item: SseItem<Data: Schema>>,
{
    // OpenAPI 3.2 describes `text/event-stream` as parsed events, so the item is
    // the event and the JSON payload hangs off `data`'s `contentSchema`.
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let mut data = SchemaObject {
            ty: Some(TypeSet::One(SchemaType::String)),
            ..SchemaObject::default()
        };
        data.content_media_type = Some(mime_names::APPLICATION_JSON.to_owned());
        data.content_schema = Some(Box::new(registry.resolve::<<S::Item as SseItem>::Data>()));

        let mut retry = SchemaObject {
            ty: Some(TypeSet::One(SchemaType::Integer)),
            ..SchemaObject::default()
        };
        retry.minimum = Some(0.0);

        let mut event = SchemaObject {
            ty: Some(TypeSet::One(SchemaType::Object)),
            ..SchemaObject::default()
        };
        event.required = Some(vec!["data".to_owned()]);
        event.properties = [
            (
                "data".to_owned(),
                kynos_openapi::Schema::Object(Box::new(data)),
            ),
            // The format gives no type to any field but `retry`.
            (
                "event".to_owned(),
                kynos_openapi::Schema::of_type(SchemaType::String),
            ),
            (
                "id".to_owned(),
                kynos_openapi::Schema::of_type(SchemaType::String),
            ),
            (
                "retry".to_owned(),
                kynos_openapi::Schema::Object(Box::new(retry)),
            ),
        ]
        .into_iter()
        .collect();

        ok(
            mime_names::TEXT_EVENT_STREAM,
            kynos_openapi::MediaType::sequential(kynos_openapi::Schema::Object(Box::new(event))),
        )
    }
}

// Private: it serves only `leading_member`.
mod leading;

#[cfg(test)]
mod tests;
