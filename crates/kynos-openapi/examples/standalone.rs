//! Building and checking a description with no server anywhere in sight.
//!
//! ```text
//! cargo run -p kynos-openapi --example standalone --no-default-features \
//!   --features openapi32,yaml
//! ```
//!
//! `kynos-openapi` is a separate crate for a reason, and this file is the
//! reason: the document model is runtime-free. Nothing here touches tokio,
//! hyper, or the `kynos` facade — which is what makes the model usable by a
//! linter, a code generator, a migration tool, or a test that reads somebody
//! else's description.
//!
//! Three things are worth noticing:
//!
//! * **`Validator` is separate from the model.** Constructing an invalid
//!   document is allowed, because a document that arrived from outside has to
//!   round-trip whether or not it is valid — reporting on it is a second step
//!   and a second type. Rejecting at construction would make the model lossy in
//!   order to enforce a rule the model does not have.
//! * **`emit` refuses rather than downgrades.** Asking a 3.2 document for its
//!   3.1 form fails when it uses a construct 3.1 cannot express. A document
//!   silently missing an operation is worse than an error, because nothing
//!   downstream can tell it happened.
//! * **The two emitters are the same document.** JSON for tools, YAML for
//!   humans reading a diff.

use kynos_openapi::{
    Document, Info, Method, Operation, PathItem, PathTemplate, Response, Responses, Schema,
    SpecVersion, model::schema::types::SchemaType, validate::Validator,
};

/// One operation, built by hand.
///
/// This is what the `kynos` facade derives from a handler signature. Seeing it
/// spelled out is the point: the model is an ordinary data structure, so a tool
/// that has no handlers can still produce a description.
fn list_orders() -> Operation {
    Operation {
        operation_id: Some("listOrders".to_owned()),
        summary: Some("Lists orders".to_owned()),
        tags: vec!["orders".to_owned()],
        responses: Responses::new().with(
            200,
            Response::with_content(
                "Every order the caller may see",
                "application/json",
                order_list(),
            ),
        ),
        ..Operation::default()
    }
}

/// An array of order identifiers.
fn order_list() -> kynos_openapi::MediaType {
    let mut items = Schema::of_type(SchemaType::Integer);
    if let Schema::Object(object) = &mut items {
        object.format = Some("uint64".to_owned());
        object.minimum = Some(0.0);
    }

    let mut array = Schema::of_type(SchemaType::Array);
    if let Schema::Object(object) = &mut array {
        object.items = Some(Box::new(items));
    }

    kynos_openapi::MediaType::new(array)
}

/// A search operation using the 3.2-only `QUERY` method.
///
/// The construct that makes `emit(V3_1)` refuse further down. 3.1 has no Path
/// Item field for `QUERY`, so there is nowhere honest to put this.
fn search_orders() -> Operation {
    Operation {
        operation_id: Some("searchOrders".to_owned()),
        responses: Responses::new().with(200, Response::new("Orders matching the filter")),
        ..Operation::default()
    }
}

fn main() {
    let mut document = Document::new(SpecVersion::V3_2, Info::new("Orders", "2.0.0"));

    let orders = PathTemplate::parse("/orders").expect("a valid path template");
    document.paths.insert(
        &orders,
        PathItem::new()
            .with_operation(Method::Get, list_orders())
            .with_operation(Method::Query, search_orders()),
    );

    // Checked, not enforced at construction. Everything above would have been
    // accepted regardless, which is what lets this same model hold a document
    // that arrived from somewhere else.
    let violations = Validator::new(SpecVersion::V3_2).validate(&document);
    if violations.is_empty() {
        println!("the description is valid at 3.2");
    }
    for violation in &violations {
        println!("violation: {violation}");
    }

    println!("{}", document.to_json().expect("serializable"));

    // For a human reading a diff rather than a generator reading a schema.
    println!("{}", document.to_yaml().expect("serializable"));

    // The refusal. `QUERY` has no 3.1 Path Item field, so this is an error and
    // not a 3.1 document with one operation quietly missing.
    match document.emit(SpecVersion::V3_1) {
        Ok(_) => println!("unexpected: 3.1 accepted a QUERY operation"),
        Err(error) => println!("3.1 refused, correctly: {error}"),
    }
}
