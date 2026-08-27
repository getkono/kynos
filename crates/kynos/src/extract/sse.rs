//! What a reconnecting Server-Sent Events client sends.
//!
//! The receive half of [`response::stream::sse`](crate::response::stream::sse).
//! The send half writes an event's `id`; this is how it comes back.
//!
//! # What Kynos does not own
//!
//! Retention, replay and de-duplication. Which events are still available, how
//! far back a resume may reach, and whether an id is even still meaningful are
//! questions about a service's event store, and Kynos has none. Making the
//! header reachable is the whole of the framework's part: an application reads
//! it and decides what to send.

use std::convert::Infallible;

use kynos_openapi::{Parameter, Schema, model::schema::types::SchemaType};

use crate::{
    extract::{FromRequestParts, describe::Describe},
    http::Parts,
    router::operation::OperationCx,
};

/// The field name, spelled once.
///
/// Both halves of this module read it: the extractor to find the value, and
/// `describe` to declare it. `Last-Event-ID` is spelled by the HTML standard's
/// `EventSource` section, and header names are case-insensitive, so the casing
/// here is what a reader of the description sees rather than what matching
/// depends on.
const LAST_EVENT_ID: &str = "Last-Event-ID";

/// The id of the last event a reconnecting client received.
///
/// `None` when the client is connecting for the first time — which is the
/// common case, and the reason this is an `Option` rather than a rejection. A
/// browser's `EventSource` sends the field only after it has seen an `id`, so
/// an absent value is a new subscriber rather than a malformed request.
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
/// A value this cannot read is one no resume can be calculated from, and the
/// protocol already defines what to send then: the stream from wherever the
/// application chooses to start it. Rejecting instead would put a status on
/// every operation that reads the field, which
/// [`conformance`](crate::test) would then require some request to actually
/// produce — and the only client that can produce it is one that did not get
/// its id from this service.
///
/// Decoded as UTF-8 rather than through
/// [`HeaderValue::to_str`](crate::http::HeaderValue::to_str), which admits
/// visible ASCII alone. [`Event::id`](crate::response::stream::sse::Event::id)
/// is a `String` and accepts any UTF-8, so an id this service minted itself can
/// contain a character `to_str` refuses — and a client returning one would
/// otherwise be told it had sent nothing.
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

/// Declares the field it reads, the way every other parameter extractor does.
///
/// Not required: a first connection carries no id, so a description marking it
/// required would be stricter than the service and wrong about the common case.
impl Describe for LastEventId {
    fn describe(operation: &mut OperationCx<'_>) {
        operation.add_parameter(Parameter::header(
            LAST_EVENT_ID,
            Schema::of_type(SchemaType::String),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::{LAST_EVENT_ID, LastEventId};

    use crate::{
        extract::{FromRequestParts, describe::Describe},
        router::operation::OperationCx,
        schema::registry::Registry,
    };

    /// Extracts from a request carrying `value` as the field, or none at all.
    async fn extracted(value: Option<&[u8]>) -> Option<String> {
        let mut parts = http::Request::new(()).into_parts().0;
        if let Some(value) = value {
            parts.headers.insert(
                LAST_EVENT_ID,
                crate::http::HeaderValue::from_bytes(value).expect("a field value"),
            );
        }

        let LastEventId(id) = LastEventId::from_request_parts(&mut parts, &())
            .await
            .expect("infallible");
        id
    }

    #[tokio::test]
    async fn a_first_connection_carries_nothing() {
        assert_eq!(extracted(None).await, None);
    }

    #[tokio::test]
    async fn a_reconnect_carries_the_id_it_was_sent() {
        assert_eq!(extracted(Some(b"42")).await, Some("42".to_owned()));
    }

    /// The reason this decodes UTF-8 rather than calling `to_str`.
    ///
    /// `Event::id` is a `String`, so a service may mint an id `to_str` refuses
    /// — it admits visible ASCII alone. A client returning one would otherwise
    /// be told it had sent nothing and would silently replay from the start.
    #[tokio::test]
    async fn an_id_outside_ascii_survives_the_round_trip() {
        assert_eq!(
            extracted(Some("café-7".as_bytes())).await,
            Some("café-7".to_owned())
        );
    }

    /// Undecodable is the same answer as absent, deliberately.
    ///
    /// No resume can be calculated from it, and the alternative is a status on
    /// every operation that reads the field which only a client that did not
    /// get its id from this service can produce.
    #[tokio::test]
    async fn a_value_that_is_not_utf8_reads_as_absent() {
        assert_eq!(extracted(Some(&[0xff, 0xfe])).await, None);
    }

    #[test]
    fn the_field_it_reads_is_the_field_it_declares() {
        let mut registry = Registry::new();
        let mut operation = OperationCx::new(&mut registry);
        LastEventId::describe(&mut operation);
        let finished = operation.finish();

        let declared: Vec<(&str, Option<bool>)> = finished
            .parameters
            .iter()
            .filter_map(|parameter| match parameter {
                kynos_openapi::RefOr::Item(item) => Some((item.name.as_str(), item.required)),
                kynos_openapi::RefOr::Ref(_) => None,
            })
            .collect();

        // Declared, and not marked required. A first connection carries no id,
        // so requiring it would make the description stricter than the service
        // -- and the keyword is left unset rather than written as `false`,
        // which is what `HeaderParams::parameters` does for every other header
        // parameter Kynos declares.
        assert_eq!(declared, vec![(LAST_EVENT_ID, None)]);
    }
}
