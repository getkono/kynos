use serde_json::json;

use crate::{
    annotation::{
        NOT_AUTHORITATIVE_ANNOTATION, OPAQUE_OPERATION_ANNOTATION, OPAQUE_ROUTES_ANNOTATION,
        Opaque, OpaqueReason, OpaqueRoute, is_authoritative, restamp_authority,
    },
    model::{
        document::{Document, SpecVersion},
        extensions::Extensions,
        info::Info,
        paths::{item::PathItem, method::Method, operation::Operation, template::PathTemplate},
    },
};

fn document() -> Document {
    Document::new(SpecVersion::V3_1, Info::new("Orders", "1.0.0"))
}

fn document_with_one_operation(operation: Operation) -> Document {
    let mut document = document();
    let template = PathTemplate::parse("/orders").expect("valid");
    document.paths.insert(
        &template,
        PathItem::new().with_operation(Method::Get, operation),
    );
    document
}

#[test]
fn a_marker_deduplicates_its_reasons() {
    let marker = Opaque::new(OpaqueReason::UntypedLayer)
        .with_reason(OpaqueReason::UntypedLayer)
        .with_reason(OpaqueReason::UntypedHandler);
    assert_eq!(
        marker.reasons,
        [OpaqueReason::UntypedLayer, OpaqueReason::UntypedHandler]
    );
}

#[test]
fn absorbing_unions_reasons_and_keeps_the_first_note() {
    let mut marker = Opaque::new(OpaqueReason::UntypedLayer).with_note("outer");
    marker.absorb(&Opaque::new(OpaqueReason::UntypedHandler).with_note("inner"));

    assert_eq!(
        marker.reasons,
        [OpaqueReason::UntypedLayer, OpaqueReason::UntypedHandler]
    );
    assert_eq!(marker.note.as_deref(), Some("outer"));
}

#[test]
fn applying_a_marker_merges_with_one_already_present() {
    let mut operation = Operation::new("listOrders");
    Opaque::new(OpaqueReason::UntypedLayer).apply_to(&mut operation);
    Opaque::new(OpaqueReason::ProtocolUpgrade).apply_to(&mut operation);

    let marker = Opaque::of(&operation).expect("well-formed");
    assert_eq!(
        marker.reasons,
        [OpaqueReason::UntypedLayer, OpaqueReason::ProtocolUpgrade]
    );
}

#[test]
fn an_unmarked_operation_reads_as_absent() {
    let operation = Operation::new("listOrders");
    assert!(!Opaque::is_annotated(&operation));
    assert_eq!(Opaque::of(&operation), None);
}

#[test]
fn a_malformed_marker_is_annotated_but_unreadable() {
    let mut operation = Operation::new("listOrders");
    operation
        .extensions
        .insert(OPAQUE_OPERATION_ANNOTATION, json!("not an object"));

    assert!(Opaque::is_annotated(&operation));
    assert_eq!(Opaque::of(&operation), None);
}

#[test]
fn reasons_serialize_in_kebab_case() {
    let value =
        serde_json::to_value(Opaque::new(OpaqueReason::UntypedRoute)).expect("serializable");
    assert_eq!(value, json!({ "reasons": ["untyped-route"] }));
}

#[test]
fn routes_accumulate_on_the_document() {
    let mut document = document();
    OpaqueRoute::new("/assets/{*path}", OpaqueReason::UntypedRoute)
        .with_prefix("/assets")
        .with_methods(["GET", "HEAD"])
        .append_to(&mut document);
    OpaqueRoute::new("/socket", OpaqueReason::ProtocolUpgrade).append_to(&mut document);

    let routes = OpaqueRoute::all(&document).expect("well-formed");
    assert_eq!(routes.len(), 2);
    assert_eq!(routes[0].prefix.as_deref(), Some("/assets"));
    assert_eq!(routes[0].methods, ["GET", "HEAD"]);
    assert_eq!(routes[1].reason, OpaqueReason::ProtocolUpgrade);
}

#[test]
fn an_absent_route_annotation_reads_as_an_empty_list() {
    let document = document();
    assert!(!OpaqueRoute::is_annotated(&document));
    assert_eq!(OpaqueRoute::all(&document), Some(Vec::new()));
}

#[test]
fn a_malformed_route_annotation_is_annotated_but_unreadable() {
    let mut document = document();
    document
        .extensions
        .insert(OPAQUE_ROUTES_ANNOTATION, json!(7));

    assert!(OpaqueRoute::is_annotated(&document));
    assert_eq!(OpaqueRoute::all(&document), None);
}

#[test]
fn a_clean_document_is_authoritative() {
    let document = document_with_one_operation(Operation::new("listOrders"));
    assert!(is_authoritative(&document));
}

#[test]
fn an_opaque_operation_costs_the_document_its_authority() {
    let mut operation = Operation::new("listOrders");
    Opaque::new(OpaqueReason::UntypedLayer).apply_to(&mut operation);
    let document = document_with_one_operation(operation);

    assert!(!is_authoritative(&document));
}

#[test]
fn an_opaque_route_costs_the_document_its_authority() {
    let mut document = document_with_one_operation(Operation::new("listOrders"));
    OpaqueRoute::new("/assets/{*path}", OpaqueReason::UntypedRoute).append_to(&mut document);

    assert!(!is_authoritative(&document));
}

#[test]
fn an_unreadable_route_annotation_is_not_treated_as_clean() {
    let mut document = document_with_one_operation(Operation::new("listOrders"));
    document
        .extensions
        .insert(OPAQUE_ROUTES_ANNOTATION, json!("nonsense"));

    assert!(!is_authoritative(&document));
}

#[test]
fn a_marked_webhook_operation_counts() {
    let mut operation = Operation::new("orderPlaced");
    Opaque::new(OpaqueReason::UntypedLayer).apply_to(&mut operation);
    let mut document = document();
    document.webhooks.insert(
        "orderPlaced".to_owned(),
        PathItem::new().with_operation(Method::Post, operation),
    );

    assert!(!is_authoritative(&document));
}

#[test]
fn restamping_adds_and_removes_the_summary() {
    let mut document = document_with_one_operation(Operation::new("listOrders"));

    restamp_authority(&mut document);
    assert_eq!(document.extensions.get(NOT_AUTHORITATIVE_ANNOTATION), None);

    OpaqueRoute::new("/assets/{*path}", OpaqueReason::UntypedRoute).append_to(&mut document);
    restamp_authority(&mut document);
    assert_eq!(
        document.extensions.get(NOT_AUTHORITATIVE_ANNOTATION),
        Some(&json!(true))
    );

    // A stamp that is no longer earned does not survive.
    document.extensions = Extensions::new();
    restamp_authority(&mut document);
    assert_eq!(document.extensions.get(NOT_AUTHORITATIVE_ANNOTATION), None);
}
