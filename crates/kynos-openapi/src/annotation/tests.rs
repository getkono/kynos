use serde_json::json;

use crate::{
    annotation::{
        NOT_AUTHORITATIVE_ANNOTATION, OPAQUE_OPERATION_ANNOTATION, OPAQUE_ROUTES_ANNOTATION,
        Opaque, OpaqueReason, OpaqueRoute,
    },
    model::{
        callback::Callback,
        document::{Document, SpecVersion},
        extensions::Extensions,
        info::Info,
        paths::{item::PathItem, method::Method, operation::Operation, template::PathTemplate},
        reference::RefOr,
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
    Opaque::new(OpaqueReason::UntypedLayer)
        .apply_to(&mut operation)
        .expect("nothing to conflict with");
    Opaque::new(OpaqueReason::ProtocolUpgrade)
        .apply_to(&mut operation)
        .expect("the existing marker is readable");

    let marker = Opaque::of(&operation)
        .expect("well-formed")
        .expect("present");
    assert_eq!(
        marker.reasons,
        [OpaqueReason::UntypedLayer, OpaqueReason::ProtocolUpgrade]
    );
}

#[test]
fn an_unmarked_operation_reads_as_absent() {
    let operation = Operation::new("listOrders");
    assert!(!Opaque::is_annotated(&operation));
    assert_eq!(Opaque::of(&operation), Ok(None));
}

#[test]
fn a_malformed_marker_is_annotated_but_unreadable() {
    let mut operation = Operation::new("listOrders");
    operation
        .extensions
        .insert(OPAQUE_OPERATION_ANNOTATION, json!("not an object"));

    assert!(Opaque::is_annotated(&operation));
    assert!(Opaque::of(&operation).is_err());
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
        .append_to(&mut document)
        .expect("nothing to conflict with");
    OpaqueRoute::new("/socket", OpaqueReason::ProtocolUpgrade)
        .append_to(&mut document)
        .expect("nothing to conflict with");

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
    assert_eq!(OpaqueRoute::all(&document), Ok(Vec::new()));
}

#[test]
fn a_malformed_route_annotation_is_annotated_but_unreadable() {
    let mut document = document();
    document
        .extensions
        .insert(OPAQUE_ROUTES_ANNOTATION, json!(7));

    assert!(OpaqueRoute::is_annotated(&document));
    assert!(OpaqueRoute::all(&document).is_err());
}

#[test]
fn a_clean_document_is_authoritative() {
    let document = document_with_one_operation(Operation::new("listOrders"));
    assert!(document.is_authoritative());
}

#[test]
fn an_opaque_operation_costs_the_document_its_authority() {
    let mut operation = Operation::new("listOrders");
    Opaque::new(OpaqueReason::UntypedLayer)
        .apply_to(&mut operation)
        .expect("nothing to conflict with");
    let document = document_with_one_operation(operation);

    assert!(!document.is_authoritative());
}

#[test]
fn an_opaque_route_costs_the_document_its_authority() {
    let mut document = document_with_one_operation(Operation::new("listOrders"));
    OpaqueRoute::new("/assets/{*path}", OpaqueReason::UntypedRoute)
        .append_to(&mut document)
        .expect("nothing to conflict with");

    assert!(!document.is_authoritative());
}

#[test]
fn an_unreadable_route_annotation_is_not_treated_as_clean() {
    let mut document = document_with_one_operation(Operation::new("listOrders"));
    document
        .extensions
        .insert(OPAQUE_ROUTES_ANNOTATION, json!("nonsense"));

    assert!(!document.is_authoritative());
}

#[test]
fn a_marked_webhook_operation_counts() {
    let mut operation = Operation::new("orderPlaced");
    Opaque::new(OpaqueReason::UntypedLayer)
        .apply_to(&mut operation)
        .expect("nothing to conflict with");
    let mut document = document();
    document.webhooks.insert(
        "orderPlaced".to_owned(),
        PathItem::new().with_operation(Method::Post, operation),
    );

    assert!(!document.is_authoritative());
}

#[test]
fn restamping_adds_and_removes_the_summary() {
    let mut document = document_with_one_operation(Operation::new("listOrders"));

    document.restamp_authority();
    assert_eq!(document.extensions.get(NOT_AUTHORITATIVE_ANNOTATION), None);

    OpaqueRoute::new("/assets/{*path}", OpaqueReason::UntypedRoute)
        .append_to(&mut document)
        .expect("nothing to conflict with");
    document.restamp_authority();
    assert_eq!(
        document.extensions.get(NOT_AUTHORITATIVE_ANNOTATION),
        Some(&json!(true))
    );

    // A stamp that is no longer earned does not survive.
    document.extensions = Extensions::new();
    document.restamp_authority();
    assert_eq!(document.extensions.get(NOT_AUTHORITATIVE_ANNOTATION), None);
}

#[test]
fn a_reason_from_a_newer_writer_round_trips_rather_than_failing() {
    let mut operation = Operation::new("listOrders");
    operation.extensions.insert(
        OPAQUE_OPERATION_ANNOTATION,
        json!({ "reasons": ["untyped-layer", "something-newer"] }),
    );

    let marker = Opaque::of(&operation)
        .expect("an unknown reason is not malformed")
        .expect("present");
    assert_eq!(
        marker.reasons,
        [
            OpaqueReason::UntypedLayer,
            OpaqueReason::Unrecognized("something-newer".to_owned())
        ]
    );

    // And writing the document back out must not quietly drop it.
    Opaque::new(OpaqueReason::ProtocolUpgrade)
        .apply_to(&mut operation)
        .expect("readable");
    assert_eq!(
        operation.extensions.get(OPAQUE_OPERATION_ANNOTATION),
        Some(&json!({
            "reasons": ["untyped-layer", "something-newer", "protocol-upgrade"]
        }))
    );
}

#[test]
fn an_unreadable_record_is_refused_rather_than_overwritten() {
    let mut document = document();
    document
        .extensions
        .insert(OPAQUE_ROUTES_ANNOTATION, json!("nonsense"));

    let error = OpaqueRoute::new("/assets/{*path}", OpaqueReason::UntypedRoute)
        .append_to(&mut document)
        .expect_err("the existing list is unreadable");
    assert_eq!(error.name, OPAQUE_ROUTES_ANNOTATION);

    // The record someone else left is still there.
    assert_eq!(
        document.extensions.get(OPAQUE_ROUTES_ANNOTATION),
        Some(&json!("nonsense"))
    );

    let mut operation = Operation::new("listOrders");
    operation
        .extensions
        .insert(OPAQUE_OPERATION_ANNOTATION, json!(7));
    assert!(
        Opaque::new(OpaqueReason::UntypedLayer)
            .apply_to(&mut operation)
            .is_err()
    );
    assert_eq!(
        operation.extensions.get(OPAQUE_OPERATION_ANNOTATION),
        Some(&json!(7))
    );
}

/// A callback is a path item in its own right, so a waiver taken inside one
/// costs the document its authority exactly as any other does.
#[test]
fn an_opaque_callback_operation_counts() {
    let mut callback_operation = Operation::new("orderShipped");
    Opaque::new(OpaqueReason::UntypedLayer)
        .apply_to(&mut callback_operation)
        .expect("nothing to conflict with");

    let mut callback = Callback::new();
    callback.0.insert(
        "{$request.body#/callbackUrl}".to_owned(),
        RefOr::Item(PathItem::new().with_operation(Method::Post, callback_operation)),
    );

    let mut operation = Operation::new("placeOrder");
    operation
        .callbacks
        .insert("onShipped".to_owned(), RefOr::Item(callback));

    let document = document_with_one_operation(operation);
    assert!(!document.is_authoritative());
}

#[test]
fn a_reason_renders_as_it_is_spelled_on_the_wire() {
    assert_eq!(OpaqueReason::UntypedLayer.to_string(), "untyped-layer");
    assert_eq!(
        OpaqueReason::Unrecognized("something-newer".to_owned()).to_string(),
        "something-newer"
    );
}

/// Every reason, and the token it is spelled with.
///
/// An exhaustive match, so a reason added to the enum stops this file compiling
/// until its wire spelling is written down — and a count beside it, so one
/// added without a row fails rather than joining a silent majority.
///
/// `docs/testing.md` asks for this of every closed set. `OpaqueReason` had
/// neither guard, which made it the set the obligation had missed.
#[test]
fn every_reason_carries_the_token_the_record_spells() {
    const SOURCE: &str = include_str!("mod.rs");

    /// The spelling, by an exhaustive match rather than by `as_str`.
    ///
    /// Written out a second time on purpose: reading `as_str` back would agree
    /// with it by construction, including wherever it is wrong.
    fn spelled(reason: &OpaqueReason) -> &str {
        match reason {
            OpaqueReason::UntypedLayer => "untyped-layer",
            OpaqueReason::UntypedRoute => "untyped-route",
            OpaqueReason::UntypedHandler => "untyped-handler",
            OpaqueReason::ProtocolUpgrade => "protocol-upgrade",
            OpaqueReason::StaticAssets => "static-assets",
            OpaqueReason::Unrecognized(reason) => reason,
        }
    }

    let every = [
        OpaqueReason::UntypedLayer,
        OpaqueReason::UntypedRoute,
        OpaqueReason::UntypedHandler,
        OpaqueReason::ProtocolUpgrade,
        OpaqueReason::StaticAssets,
    ];

    for reason in &every {
        assert_eq!(reason.as_str(), spelled(reason));

        // And the token survives a round trip, which is what a document read
        // back by another build depends on.
        let json = serde_json::to_string(reason).expect("serializable");
        assert_eq!(json, format!("\"{}\"", spelled(reason)));
        assert_eq!(
            serde_json::from_str::<OpaqueReason>(&json).expect("readable"),
            *reason
        );
    }

    // Counted against the source, so a sixth reason without a row fails the
    // build. The needle is an `as_str` arm that yields a literal, which every
    // named variant has and `Unrecognized` -- which yields its own text -- does
    // not. Spelled in two pieces, since a contiguous literal would count itself.
    let declared = SOURCE.matches(concat!(" => ", "\"")).count();

    assert_eq!(
        every.len(),
        declared,
        "`OpaqueReason` has {declared} named variant(s) and {} have a row; a reason added \
         without one is a record a consumer cannot read back",
        every.len()
    );
}
