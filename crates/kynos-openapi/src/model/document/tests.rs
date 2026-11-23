use super::{Document, SpecVersion};
use crate::model::{
    info::Info,
    parameter::{Parameter, ParameterIn},
    reference::{Ref, RefOr, UnresolvedRef},
    schema::{Schema, dialect::OAS_DIALECT},
};

fn document() -> Document {
    Document::new(SpecVersion::V3_1, Info::new("Orders", "1.0.0"))
}

#[test]
fn a_new_document_declares_its_version() {
    assert_eq!(document().openapi, "3.1.2");
    assert_eq!(document().spec_version(), Some(SpecVersion::V3_1));
}

#[test]
fn the_version_is_matched_on_major_and_minor_only() {
    let mut doc = document();
    doc.openapi = "3.1.0".to_owned();
    assert_eq!(doc.spec_version(), Some(SpecVersion::V3_1));
}

#[test]
fn an_unmodelled_version_is_reported_as_unknown() {
    let mut doc = document();
    doc.openapi = "3.0.4".to_owned();
    assert_eq!(doc.spec_version(), None);
}

#[test]
fn the_dialect_defaults_to_the_oas_dialect() {
    assert_eq!(document().effective_dialect(), OAS_DIALECT);
}

#[test]
fn three_one_does_not_claim_three_two_support() {
    assert!(!SpecVersion::V3_1.supports_3_2());
}

fn parsed(json: serde_json::Value) -> Document {
    serde_json::from_value(json).expect("document")
}

/// Each operation `doc` yields, as `(pointer, path, method)`.
fn operations(doc: &Document) -> Vec<(String, Option<&str>, &str)> {
    doc.operations()
        .map(|found| (found.pointer, found.path, found.method))
        .collect()
}

#[test]
fn operations_are_every_one_written_anywhere_each_once_where_it_is_written() {
    let doc = parsed(serde_json::json!({
        "openapi": "3.1.2",
        "info": { "title": "Orders", "version": "1" },
        "paths": {
            "/orders": {
                "summary": "not an operation",
                "parameters": [],
                "post": {
                    "callbacks": {
                        "done": { "{$request.body#/url}": { "post": {} } }
                    }
                },
                "get": {}
            },
            "/shared": { "$ref": "#/components/pathItems/Shared" }
        },
        "webhooks": { "created": { "put": {} } },
        "components": {
            "pathItems": { "Shared": { "delete": {} } },
            "callbacks": { "Later": { "{$url}": { "get": {} } } }
        }
    }));

    assert_eq!(
        operations(&doc),
        [
            ("#/paths/~1orders/get".to_owned(), Some("/orders"), "GET"),
            ("#/paths/~1orders/post".to_owned(), Some("/orders"), "POST"),
            (
                "#/paths/~1orders/post/callbacks/done/{$request.body#~1url}/post".to_owned(),
                None,
                "POST"
            ),
            ("#/webhooks/created/put".to_owned(), None, "PUT"),
            (
                "#/components/pathItems/Shared/delete".to_owned(),
                None,
                "DELETE"
            ),
            (
                "#/components/callbacks/Later/{$url}/get".to_owned(),
                None,
                "GET"
            ),
        ]
    );
}

#[test]
fn an_operation_is_yielded_with_the_path_item_holding_it() {
    let doc = parsed(serde_json::json!({
        "openapi": "3.1.2",
        "info": { "title": "Orders", "version": "1" },
        "paths": { "/orders": { "summary": "Orders", "get": { "operationId": "list" } } }
    }));

    let found = doc.operations().next().expect("one operation");
    assert_eq!(found.item.summary.as_deref(), Some("Orders"));
    assert_eq!(found.operation.operation_id.as_deref(), Some("list"));
}

#[cfg(feature = "openapi32")]
#[test]
fn additional_operations_follow_the_fixed_ones_under_their_own_token() {
    let doc = parsed(serde_json::json!({
        "openapi": "3.2.0",
        "info": { "title": "Files", "version": "1" },
        "paths": {
            "/files": {
                "additionalOperations": { "COPY": {}, "LOCK": {} },
                "query": {},
                "get": {}
            }
        }
    }));

    assert_eq!(
        operations(&doc),
        [
            ("#/paths/~1files/get".to_owned(), Some("/files"), "GET"),
            ("#/paths/~1files/query".to_owned(), Some("/files"), "QUERY"),
            (
                "#/paths/~1files/additionalOperations/COPY".to_owned(),
                Some("/files"),
                "COPY"
            ),
            (
                "#/paths/~1files/additionalOperations/LOCK".to_owned(),
                Some("/files"),
                "LOCK"
            ),
        ]
    );
}

/// A document whose `components.parameters` holds `entries`, as JSON.
fn with_parameters(entries: &serde_json::Value) -> Document {
    parsed(serde_json::json!({
        "openapi": "3.1.2",
        "info": { "title": "Orders", "version": "1" },
        "components": { "parameters": entries }
    }))
}

fn parameter_ref(location: &str) -> RefOr<Parameter> {
    RefOr::Ref(Ref::new(location))
}

#[test]
fn a_reference_is_followed_through_a_chain_to_the_object() {
    let doc = with_parameters(&serde_json::json!({
        "Alias": { "$ref": "#/components/parameters/Limit" },
        "Limit": { "name": "limit", "in": "query", "schema": {} }
    }));

    let alias = parameter_ref("#/components/parameters/Alias");
    let found = doc.resolve(&alias).expect("resolved");
    assert_eq!(found.name, "limit");
}

#[test]
fn an_inline_entry_resolves_to_itself() {
    let doc = with_parameters(&serde_json::json!({}));
    let inline = RefOr::Item(Parameter::new(
        "limit",
        ParameterIn::Query,
        Schema::default(),
    ));
    assert_eq!(doc.resolve(&inline), Ok(inline.as_item().expect("inline")));
}

#[test]
fn a_reference_this_document_cannot_follow_is_not_local() {
    let doc = with_parameters(&serde_json::json!({
        "Limit": { "name": "limit", "in": "query", "schema": {} }
    }));

    for location in [
        "other.yaml#/components/parameters/Limit",
        "#/components/headers/Limit",
        "#/components/parameters/Limit/schema",
        "#/components/parameters/Li~0mit",
        "#/components/parameters/Li%6Dit",
    ] {
        assert_eq!(
            doc.resolve(&parameter_ref(location)),
            Err(UnresolvedRef::NotLocal),
            "{location}"
        );
    }
}

#[test]
fn a_reference_to_an_undeclared_component_is_missing_anywhere_in_the_chain() {
    let doc = with_parameters(&serde_json::json!({
        "Alias": { "$ref": "#/components/parameters/Gone" }
    }));

    for location in [
        "#/components/parameters/Gone",
        "#/components/parameters/Alias",
    ] {
        assert_eq!(
            doc.resolve(&parameter_ref(location)),
            Err(UnresolvedRef::Missing),
            "{location}"
        );
    }
}

#[test]
fn a_cycle_of_references_is_reported_rather_than_followed_forever() {
    let doc = with_parameters(&serde_json::json!({
        "A": { "$ref": "#/components/parameters/B" },
        "B": { "$ref": "#/components/parameters/A" },
        "Self": { "$ref": "#/components/parameters/Self" }
    }));

    for location in ["#/components/parameters/A", "#/components/parameters/Self"] {
        assert_eq!(
            doc.resolve(&parameter_ref(location)),
            Err(UnresolvedRef::Cycle),
            "{location}"
        );
    }
}

/// A document whose `components.schemas` holds `schemas`, as JSON.
fn with_schemas(schemas: &serde_json::Value) -> Document {
    parsed(serde_json::json!({
        "openapi": "3.1.2",
        "info": { "title": "Orders", "version": "1" },
        "components": { "schemas": schemas }
    }))
}

#[test]
fn a_schema_ref_is_followed_through_a_chain_to_the_schema_without_one() {
    let doc = with_schemas(&serde_json::json!({
        "Alias": { "$ref": "#/components/schemas/Order", "description": "beside" },
        "Order": { "type": "object" }
    }));

    let alias = Schema::component("Alias");
    let found = doc.resolve_schema(&alias).expect("resolved");
    assert_eq!(found, &doc.components.schemas["Order"]);
}

#[test]
fn a_schema_without_a_ref_resolves_to_itself() {
    let doc = with_schemas(&serde_json::json!({}));
    for schema in [Schema::any(), Schema::default()] {
        assert_eq!(doc.resolve_schema(&schema), Ok(&schema));
    }
}

#[test]
fn a_schema_ref_fails_as_a_parameter_ref_does() {
    let doc = with_schemas(&serde_json::json!({
        "Order": { "type": "object" },
        "Alias": { "$ref": "#/components/schemas/Gone" },
        "A": { "$ref": "#/components/schemas/B" },
        "B": { "$ref": "#/components/schemas/A" }
    }));

    for (location, expected) in [
        (
            "#/components/schemas/Order/properties/id",
            UnresolvedRef::NotLocal,
        ),
        ("#/$defs/Order", UnresolvedRef::NotLocal),
        ("#/components/schemas/Alias", UnresolvedRef::Missing),
        ("#/components/schemas/A", UnresolvedRef::Cycle),
    ] {
        assert_eq!(
            doc.resolve_schema(&Schema::reference(location)),
            Err(expected),
            "{location}"
        );
    }
}
