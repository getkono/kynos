//! What a reconnecting Server-Sent Events client sends.
//!
//! The receive half of [`response::stream::sse`](crate::response::stream::sse).
//! The send half writes an event's `id`; this is how it comes back.
//!
//! # What Kynos does not own
//!
//! Retention, replay and de-duplication: those belong to the application's
//! event store. Kynos only makes the header reachable.

use std::convert::Infallible;

use kynos_openapi::{Parameter, Schema, model::schema::types::SchemaType};

use crate::{
    extract::{FromRequestParts, describe::Describe},
    http::Parts,
    router::operation::OperationCx,
};

/// The field name as the HTML standard spells it, read by both the extractor
/// and `describe`; matching is case-insensitive.
const LAST_EVENT_ID: &str = "Last-Event-ID";

/// The id of the last event a reconnecting client received.
///
/// `None` when the client is connecting for the first time: `EventSource`
/// sends the field only after it has seen an `id`.
///
/// ```no_run
/// use kynos::{extract::sse::LastEventId, response::stream::sse::Sse};
/// # struct Feed;
/// # impl Feed {
/// #     fn resuming_after(_id: Option<&str>) -> Self { Self }
/// # }
///
/// #[kynos::get("/events")]
/// async fn events(LastEventId(resume): LastEventId) -> Sse<Feed> {
///     // Which events are still available, and how far back a resume may
///     // reach, are the application's to answer.
///     Sse::new(Feed::resuming_after(resume.as_deref()))
/// }
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LastEventId(pub Option<String>);

impl LastEventId {
    /// The id, if the client sent one.
    #[must_use]
    pub fn as_deref(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

/// Infallible, and undecodable is the same answer as absent.
///
/// An unreadable id cannot be resumed from, so the stream starts wherever the
/// application chooses, as for a first connection.
///
/// Decoded as UTF-8 rather than visible ASCII, since
/// [`Event::id`](crate::response::stream::sse::Event::id) accepts any UTF-8.
impl<C: Sync> FromRequestParts<C> for LastEventId {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        Ok(Self(
            parts
                .headers
                .get(LAST_EVENT_ID)
                .and_then(|value| std::str::from_utf8(value.as_bytes()).ok())
                .map(str::to_owned),
        ))
    }
}

/// Declares the header as an optional parameter.
impl Describe for LastEventId {
    fn describe(operation: &mut OperationCx<'_>) {
        operation.add_parameter(Parameter::header(
            LAST_EVENT_ID,
            Schema::of_type(SchemaType::String),
        ));
    }
}

#[cfg(test)]
mod tests;
