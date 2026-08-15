//! Server-Sent Events.

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use kynos_openapi::{
    SchemaObject,
    model::schema::types::{SchemaType, TypeSet},
};

use crate::{
    http::{HeaderValue, Response, body::Body, header},
    response::{IntoResponse, Responses},
    schema::{Schema, registry::Registry},
};

/// The error any body reports, whatever the stream's own error type was.
type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// A Server-Sent Events response.
///
/// Requires `openapi32`. Under OpenAPI 3.1 an event stream can only be
/// described as an opaque string, which says nothing useful about the events;
/// 3.2's `itemSchema` is what makes each event describable. Kynos would rather
/// not compile than emit a description that lies about your stream.
///
/// ```no_run
/// # #[cfg(feature = "openapi32")]
/// # {
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
/// # }
/// ```
#[derive(Debug)]
pub struct Sse<S> {
    /// The stream of events.
    pub events: S,
    keep_alive: Option<KeepAlive>,
}

impl<S> Sse<S> {
    /// Creates an event stream without keep-alive messages.
    pub fn new(events: S) -> Self {
        Self {
            events,
            keep_alive: None,
        }
    }

    /// Configures periodic keep-alive comments.
    #[must_use]
    pub fn keep_alive(mut self, keep_alive: KeepAlive) -> Self {
        self.keep_alive = Some(keep_alive);
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
    pub fn new() -> Self {
        Self {
            interval: std::time::Duration::from_secs(15),
            comment: String::new(),
        }
    }

    /// Sets the interval between keep-alive messages.
    #[must_use]
    pub fn interval(mut self, interval: std::time::Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Sets the comment carried by each keep-alive message.
    #[must_use]
    pub fn comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = comment.into();
        self
    }

    /// Alias for [`comment`](Self::comment), matching axum's builder name.
    #[must_use]
    pub fn text(self, text: impl Into<String>) -> Self {
        self.comment(text)
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
    /// How long a client should wait before reconnecting, in milliseconds.
    pub retry: Option<u64>,
    /// A comment sent before the event data, if any.
    pub comment: Option<String>,
}

impl<T> Event<T> {
    /// Creates an event carrying typed data.
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

    /// Sets the client reconnection delay in milliseconds.
    #[must_use]
    pub fn retry(mut self, retry: u64) -> Self {
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

/// The events, framed as the `text/event-stream` bytes that carry them.
///
/// The stream is held boxed so that it can be polled without a projection:
/// `Pin<Box<S>>` is `Unpin` whatever `S` is, and `unsafe` is forbidden here.
struct Records<S> {
    events: Pin<Box<S>>,
    keep_alive: Option<Heartbeat>,
}

/// The timer that keeps an idle stream from being reaped.
///
/// A keep-alive is the one body whose own contract *is* a timer: nothing
/// outside this stream can know when it last produced, and the connection
/// driver cannot inspect a body to find out. `docs/architecture.md` records
/// this as one of the enumerated places the runtime is named outside `server/`.
struct Heartbeat {
    /// How long a silence may last before a comment is sent.
    interval: std::time::Duration,
    /// Boxed for the reason [`Records::events`] is: `Pin<Box<Sleep>>` is
    /// `Unpin`, and `unsafe` is forbidden here.
    sleep: Pin<Box<tokio::time::Sleep>>,
    /// The comment record, rendered once. Cloning a `Bytes` is a refcount bump.
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
/// A comment record: `: text`, one line per line of the comment, then the blank
/// line that ends it. An empty comment gives `: `, which is the heartbeat every
/// SSE client already ignores — and which is why the default carries no text.
fn heartbeat_record(comment: &str) -> bytes::Bytes {
    let mut record = String::new();
    field(&mut record, "", comment);
    record.push('\n');
    bytes::Bytes::from(record)
}

impl<S, T, E> futures_core::Stream for Records<S>
where
    S: futures_core::Stream<Item = Result<Event<T>, E>>,
    T: serde::Serialize,
    E: Into<BoxError>,
{
    type Item = Result<bytes::Bytes, BoxError>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let records = self.get_mut();

        // The events first: anything they produced resets the silence this is
        // measuring, so a busy stream never sends a comment at all.
        match records.events.as_mut().poll_next(context) {
            Poll::Ready(Some(Ok(event))) => {
                if let Some(keep_alive) = records.keep_alive.as_mut() {
                    keep_alive.restart();
                }
                return Poll::Ready(Some(encode(&event)));
            }
            Poll::Ready(Some(Err(error))) => return Poll::Ready(Some(Err(error.into()))),
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Pending => {}
        }

        let Some(keep_alive) = records.keep_alive.as_mut() else {
            return Poll::Pending;
        };

        // Both futures are now registered -- the events above returned
        // `Pending` after storing the waker, and this stores it too -- so
        // whichever fires first wakes the task and neither wakeup is lost.
        match keep_alive.sleep.as_mut().poll(context) {
            Poll::Ready(()) => {
                keep_alive.restart();
                Poll::Ready(Some(Ok(keep_alive.record.clone())))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Writes one event as a `text/event-stream` record.
///
/// The fields are `name: value` lines and the record ends with the blank line
/// that tells a client to dispatch it. The data is JSON, which is the form
/// [`Responses`] describes it in.
fn encode<T: serde::Serialize>(event: &Event<T>) -> Result<bytes::Bytes, BoxError> {
    let data = serde_json::to_string(&event.data)?;

    let mut record = String::new();
    // A comment precedes the event it belongs to; a client ignores it, and a
    // proxy counts it as traffic on an otherwise idle connection.
    if let Some(comment) = &event.comment {
        field(&mut record, "", comment);
    }
    if let Some(name) = &event.event {
        field(&mut record, "event", name);
    }
    if let Some(id) = &event.id {
        field(&mut record, "id", id);
    }
    if let Some(retry) = event.retry {
        field(&mut record, "retry", &retry.to_string());
    }
    field(&mut record, "data", &data);
    record.push('\n');

    Ok(bytes::Bytes::from(record))
}

/// Writes one field, one line per line of its value.
///
/// A line break inside a value would otherwise end the field, so a multi-line
/// value is written as several fields of the same name — which is how the format
/// carries a newline at all. An empty `name` writes the comment form, `: text`.
fn field(record: &mut String, name: &str, value: &str) {
    for line in value.split('\n') {
        record.push_str(name);
        record.push_str(": ");
        record.push_str(line.strip_suffix('\r').unwrap_or(line));
        record.push('\n');
    }
}

impl<S, T, E> IntoResponse for Sse<S>
where
    S: futures_core::Stream<Item = Result<Event<T>, E>> + Send + 'static,
    T: serde::Serialize,
    E: Into<BoxError> + 'static,
{
    fn into_response(self) -> Response {
        let records = Records {
            events: Box::pin(self.events),
            keep_alive: self.keep_alive.as_ref().map(Heartbeat::new),
        };

        let mut response = Response::new(Body::from_stream(records));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream"),
        );
        response
    }
}

impl<S, T, E> Responses for Sse<S>
where
    S: futures_core::Stream<Item = Result<Event<T>, E>>,
    T: Schema,
{
    // The item is the *parsed event*, not the payload: OpenAPI 3.2 requires
    // `text/event-stream` to be described after the stream has been parsed, so
    // every field value is a string and the JSON payload is reached through
    // `contentMediaType`/`contentSchema` rather than being the item itself.
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let mut data = SchemaObject {
            ty: Some(TypeSet::One(SchemaType::String)),
            ..SchemaObject::default()
        };
        data.content_media_type = Some("application/json".to_owned());
        data.content_schema = Some(Box::new(registry.resolve::<T>()));

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
            ("event".to_owned(), text()),
            ("id".to_owned(), text()),
            (
                "retry".to_owned(),
                kynos_openapi::Schema::Object(Box::new(retry)),
            ),
        ]
        .into_iter()
        .collect();

        kynos_openapi::Responses::new().with(
            200,
            kynos_openapi::Response::with_content(
                "OK",
                "text/event-stream",
                kynos_openapi::MediaType::sequential(kynos_openapi::Schema::Object(Box::new(
                    event,
                ))),
            ),
        )
    }
}

/// The schema of a field the format gives no type to, which is every field but
/// `retry`.
fn text() -> kynos_openapi::Schema {
    kynos_openapi::Schema::Object(Box::new(SchemaObject {
        ty: Some(TypeSet::One(SchemaType::String)),
        ..SchemaObject::default()
    }))
}

#[cfg(test)]
mod tests {
    use super::{Event, encode, heartbeat_record};

    /// Re-parses a record into its `(field, value)` pairs, using a reader
    /// transcribed from the `text/event-stream` grammar rather than from
    /// [`encode`].
    ///
    /// An oracle derived from the writer would agree with it by construction,
    /// including wherever both are wrong — which is the whole of the Parser rule
    /// in `docs/testing.md`.
    fn reparse(record: &[u8]) -> Vec<(String, String)> {
        let text = std::str::from_utf8(record).expect("a UTF-8 record");
        let mut fields = Vec::new();

        for line in text.split('\n') {
            if line.is_empty() {
                // The blank line that dispatches the event.
                continue;
            }

            // The format splits on the first colon; one space after it is part
            // of the delimiter rather than of the value.
            let (name, value) = line.split_once(':').expect("a `name: value` line");
            fields.push((
                name.to_owned(),
                value.strip_prefix(' ').unwrap_or(value).to_owned(),
            ));
        }

        fields
    }

    #[test]
    fn a_keep_alive_comment_is_framed_as_a_record_a_client_ignores() {
        let record = heartbeat_record("ping");

        // An empty field name is the comment form, which every client discards.
        assert_eq!(reparse(&record), [(String::new(), "ping".to_owned())]);
        assert!(
            record.ends_with(b"\n\n"),
            "a record ends with the blank line that dispatches it"
        );
    }

    /// A line break inside a value would otherwise end the field, so a
    /// multi-line comment travels as several fields of the same name.
    #[test]
    fn a_multi_line_keep_alive_comment_is_written_one_line_per_field() {
        let record = heartbeat_record("first\nsecond");

        assert_eq!(
            reparse(&record),
            [
                (String::new(), "first".to_owned()),
                (String::new(), "second".to_owned()),
            ]
        );
    }

    /// The default carries no text, which is the shortest thing a client will
    /// still read as a live connection.
    #[test]
    fn a_keep_alive_with_no_comment_is_still_a_record() {
        let record = heartbeat_record("");

        assert_eq!(&record[..], b": \n\n");
    }

    /// The same framing an event's own fields take, so a comment and an event
    /// cannot disagree about what a record is.
    #[test]
    fn an_event_ends_with_the_blank_line_that_dispatches_it() {
        let record = encode(&Event::new(1_u8)).expect("an encodable event");

        assert!(record.ends_with(b"\n\n"));
        assert_eq!(reparse(&record), [("data".to_owned(), "1".to_owned())]);
    }

    /// Every field an event can carry, in the order the format wants them.
    ///
    /// `data` last is not cosmetic: a client dispatches on the blank line and
    /// reads the other fields as belonging to the data it has accumulated, so
    /// an `id` written after `data` belongs to the *next* event.
    #[test]
    fn an_event_writes_every_field_it_carries_in_dispatch_order() {
        let event = Event::new(vec![1_u8, 2])
            .comment("about to happen")
            .event("created")
            .id("42")
            .retry(3_000);

        assert_eq!(
            reparse(&encode(&event).expect("an encodable event")),
            [
                (String::new(), "about to happen".to_owned()),
                ("event".to_owned(), "created".to_owned()),
                ("id".to_owned(), "42".to_owned()),
                ("retry".to_owned(), "3000".to_owned()),
                ("data".to_owned(), "[1,2]".to_owned()),
            ]
        );
    }

    /// An omitted field is absent rather than empty: a client reads `id:` with
    /// no value as *clearing* the last event id, which is not what an event
    /// that never set one means.
    #[test]
    fn an_event_writes_no_field_it_did_not_carry() {
        let record = encode(&Event::new(1_u8)).expect("an encodable event");
        let written: Vec<String> = reparse(&record).into_iter().map(|(name, _)| name).collect();

        assert_eq!(written, ["data"]);
    }

    /// A newline inside a value is how the format carries a newline at all:
    /// several fields of one name, which a client rejoins with `\n`.
    ///
    /// This is where a golden-string snapshot would be worthless -- it would
    /// pin the bytes `encode` happens to write, and the question is whether a
    /// reader recovers the value.
    #[test]
    fn a_multi_line_value_travels_as_one_field_per_line() {
        let event = Event::new("first\nsecond\nthird");
        let fields = reparse(&encode(&event).expect("an encodable event"));

        // JSON keeps the breaks inside the string, so the data is one line.
        assert_eq!(
            fields,
            [("data".to_owned(), r#""first\nsecond\nthird""#.to_owned())]
        );

        // A comment is not JSON, so its breaks do reach the framing.
        let event = Event::new(0_u8).comment("first\nsecond");
        assert_eq!(
            reparse(&encode(&event).expect("an encodable event")),
            [
                (String::new(), "first".to_owned()),
                (String::new(), "second".to_owned()),
                ("data".to_owned(), "0".to_owned()),
            ]
        );
    }

    /// A CRLF in a value is one line break, not two.
    ///
    /// The format ends a line on CR, LF or CRLF, so writing the CR through
    /// would produce a stray empty field -- and an empty field is the blank
    /// line that dispatches the event, which would cut the record in half.
    #[test]
    fn a_carriage_return_does_not_become_a_second_line() {
        let event = Event::new(0_u8).comment("first\r\nsecond");

        assert_eq!(
            reparse(&encode(&event).expect("an encodable event")),
            [
                (String::new(), "first".to_owned()),
                (String::new(), "second".to_owned()),
                ("data".to_owned(), "0".to_owned()),
            ]
        );
    }

    /// An event whose data cannot be serialized is an error rather than a
    /// record, because a half-written record would desynchronize the stream.
    #[test]
    fn an_unserializable_event_is_refused_rather_than_half_written() {
        use std::collections::HashMap;

        // A map keyed by something JSON cannot spell as an object key.
        let mut data = HashMap::new();
        data.insert(vec![1_u8], "value");

        assert!(encode(&Event::new(data)).is_err());
    }
}
