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
