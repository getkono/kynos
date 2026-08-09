//! Server-Sent Events.

use crate::{
    http::Response,
    response::{IntoResponse, Responses},
    schema::{Registry, Schema},
};

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
    /// The event payload, described by `itemSchema`.
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

impl<S, T, E> IntoResponse for Sse<S>
where
    S: futures_core::Stream<Item = Result<Event<T>, E>> + Send + 'static,
    T: serde::Serialize,
    E: Into<Box<dyn std::error::Error + Send + Sync>> + 'static,
{
    fn into_response(self) -> Response {
        todo!()
    }
}

impl<S, T, E> Responses for Sse<S>
where
    S: futures_core::Stream<Item = Result<Event<T>, E>>,
    T: Schema,
{
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}
