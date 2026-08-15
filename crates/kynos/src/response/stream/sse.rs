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
}

impl<S, T, E> futures_core::Stream for Records<S>
where
    S: futures_core::Stream<Item = Result<Event<T>, E>>,
    T: serde::Serialize,
    E: Into<BoxError>,
{
    type Item = Result<bytes::Bytes, BoxError>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.get_mut().events.as_mut().poll_next(context) {
            Poll::Ready(Some(Ok(event))) => Poll::Ready(Some(encode(&event))),
            Poll::Ready(Some(Err(error))) => Poll::Ready(Some(Err(error.into()))),
            Poll::Ready(None) => Poll::Ready(None),
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
