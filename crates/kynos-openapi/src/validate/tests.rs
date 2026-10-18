use std::collections::BTreeSet;

use super::{SpecError, Validator, Violation};
use crate::model::{
    document::{Document, SpecVersion},
    info::Info,
    parameter::{Parameter, style::Style},
    paths::{item::PathItem, method::Method, operation::Operation, template::PathTemplate},
    response::{Response, Responses},
    schema::{
        Schema,
        discriminator::Discriminator,
        object::SchemaObject,
        types::{SchemaType, TypeSet},
    },
    security::requirement::SecurityRequirement,
};

fn ok_responses() -> Responses {
    Responses::new().with(200, Response::new("ok"))
}

fn document_with(paths: &[(&str, PathItem)]) -> Document {
    let mut document = Document::new(SpecVersion::V3_1, Info::new("Test", "1.0.0"));
    for (raw, item) in paths {
        let template = PathTemplate::parse(*raw).expect("valid template");
        document.paths.insert(&template, item.clone());
    }
    document
}

fn errors(document: &Document) -> Vec<SpecError> {
    Validator::new(SpecVersion::V3_1)
        .validate(document)
        .into_iter()
        .filter(|v| v.severity == super::Severity::Error)
        .map(|v| v.error)
        .collect()
}

#[test]
fn a_minimal_valid_document_has_no_errors() {
    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("health").with_responses(ok_responses()),
    );
    assert!(errors(&document_with(&[("/health", item)])).is_empty());
}

#[test]
fn duplicate_operation_ids_are_rejected() {
    let first = PathItem::new().with_operation(
        Method::Get,
        Operation::new("getThing").with_responses(ok_responses()),
    );
    let second = PathItem::new().with_operation(
        Method::Get,
        Operation::new("getThing").with_responses(ok_responses()),
    );

    let found = errors(&document_with(&[("/a", first), ("/b", second)]));
    assert!(matches!(
        found.as_slice(),
        [SpecError::DuplicateOperationId { operation_id, .. }] if operation_id == "getThing"
    ));
}

/// Every operation the document describes is validated, not only `paths`.
///
/// `references/3.1.2.md:979` and `references/3.2.0.md:680` say an
/// `operationId` "MUST be unique among **all operations described in the
/// API**". A webhook, a reusable path item and a callback all describe
/// operations, and `check_paths` iterated `document.paths` alone — so every
/// operation-level rule stopped at that boundary. `rules/opaque.rs` already
/// walks all four containers, so this was an oversight in one walk rather than
/// a position the crate had taken.
///
/// One case per container, and one per rule would be the wrong axis: what was
/// broken is the *route to* the operation, not any individual rule.
#[test]
fn an_operation_outside_paths_is_validated_too() {
    use crate::model::{callback::Callback, reference::RefOr};

    let duplicate = || {
        PathItem::new().with_operation(
            Method::Get,
            Operation::new("shared").with_responses(ok_responses()),
        )
    };

    // The one in `paths` is the first sighting; each case below adds a second
    // somewhere the walk did not reach.
    let with_second = |place: &dyn Fn(&mut Document)| {
        let mut document = document_with(&[("/users", duplicate())]);
        place(&mut document);
        errors(&document)
    };

    let webhooks = with_second(&|document| {
        document
            .webhooks
            .insert("orderPlaced".to_owned(), duplicate());
    });
    assert!(
        webhooks
            .iter()
            .any(|error| matches!(error, SpecError::DuplicateOperationId { .. })),
        "a webhook describes an operation; got {webhooks:?}"
    );

    let components = with_second(&|document| {
        document
            .components
            .path_items
            .insert("Shared".to_owned(), duplicate());
    });
    assert!(
        components
            .iter()
            .any(|error| matches!(error, SpecError::DuplicateOperationId { .. })),
        "a reusable path item describes an operation; got {components:?}"
    );

    let callbacks = with_second(&|document| {
        document.components.callbacks.insert(
            "onData".to_owned(),
            RefOr::Item(Callback::new().with("{$request.body#/url}", duplicate())),
        );
    });
    assert!(
        callbacks
            .iter()
            .any(|error| matches!(error, SpecError::DuplicateOperationId { .. })),
        "a callback describes an operation; got {callbacks:?}"
    );
}

/// An operation's own `callbacks` describe operations too.
///
/// The case above reaches a callback only through `components.callbacks`; one
/// written inline on an operation hangs off that operation instead, and is
/// just as much one of "all operations described in the API". Each case
/// carries a different operation-level rule, and the nested one shows the walk
/// descends through a callback's own operations as well.
#[test]
fn an_operation_inside_an_inline_callback_is_validated() {
    use crate::model::{callback::Callback, reference::RefOr};

    let with_callback = |operation: Operation| {
        let mut owner = Operation::new("subscribe").with_responses(ok_responses());
        owner.callbacks.insert(
            "onData".to_owned(),
            RefOr::Item(Callback::new().with(
                "{$request.body#/url}",
                PathItem::new().with_operation(Method::Post, operation),
            )),
        );
        owner
    };
    let validate = |owner: Operation| {
        Validator::new(SpecVersion::V3_1).validate(&document_with(&[(
            "/subscriptions",
            PathItem::new().with_operation(Method::Post, owner),
        )]))
    };

    let duplicate = validate(with_callback(
        Operation::new("subscribe").with_responses(ok_responses()),
    ));
    assert!(
        duplicate.iter().any(|violation| matches!(
            &violation.error,
            SpecError::DuplicateOperationId { operation_id, .. } if operation_id == "subscribe"
        ) && violation.location
            == "#/paths/~1subscriptions/post/callbacks/onData/{$request.body#~1url}/post"),
        "an inline callback describes an operation; got {duplicate:?}"
    );

    let responseless = validate(with_callback(with_callback(Operation::new("notify"))));
    assert!(
        responseless.iter().any(
            |violation| matches!(violation.error, SpecError::NoResponses)
                && violation.location
                    == "#/paths/~1subscriptions/post/callbacks/onData/{$request.body#~1url}/post\
                /callbacks/onData/{$request.body#~1url}/post"
        ),
        "a callback's own callbacks describe operations; got {responseless:?}"
    );
}

/// A referenced callback is validated once, at the component it names.
///
/// The walk above follows inline items only: following a `$ref` would visit
/// the component once per reference and report a duplicate `operationId`
/// against the component itself. Both reference positions are covered — the
/// callback itself, and a Path Item inside an inline callback.
#[test]
fn a_referenced_callback_is_not_walked_again_from_its_operation() {
    use crate::model::{
        callback::Callback,
        reference::{Ref, RefOr},
    };

    let notify = |operation_id: &str| {
        PathItem::new().with_operation(
            Method::Post,
            Operation::new(operation_id).with_responses(ok_responses()),
        )
    };
    let mut owner = Operation::new("subscribe").with_responses(ok_responses());
    owner.callbacks.insert(
        "onData".to_owned(),
        RefOr::Ref(Ref::new("#/components/callbacks/OnData")),
    );
    let mut item_ref = Callback::new();
    item_ref.items.insert(
        "{$request.body#/url}".to_owned(),
        RefOr::Ref(Ref::new("#/components/pathItems/Notify")),
    );
    owner
        .callbacks
        .insert("onItem".to_owned(), RefOr::Item(item_ref));

    let mut document = document_with(&[(
        "/subscriptions",
        PathItem::new().with_operation(Method::Post, owner),
    )]);
    document.components.callbacks.insert(
        "OnData".to_owned(),
        RefOr::Item(Callback::new().with("{$request.body#/url}", notify("onData"))),
    );
    document
        .components
        .path_items
        .insert("Notify".to_owned(), notify("onItem"));

    let found = errors(&document);
    assert!(
        found.is_empty(),
        "a referenced callback is validated at its component alone; got {found:?}"
    );
}

/// A callback is refused exactly when it would not read back as itself.
///
/// `Callback` writes its Path Items and its extensions into one object and
/// reads a key beginning `x-` back as an extension, the field pattern
/// `references/3.1.2.md:4206` gives them — so an item keyed that way returns as
/// an extension, and the operations under it are gone. `X-hook` pins that the
/// prefix is matched as the reader matches it: case-sensitively.
#[test]
fn a_callback_is_refused_exactly_when_it_does_not_round_trip() {
    use crate::model::{callback::Callback, reference::RefOr};

    for key in ["{$request.body#/url}", "X-hook", "x-hook"] {
        let notify = PathItem::new().with_operation(
            Method::Post,
            Operation::new("notify").with_responses(ok_responses()),
        );
        let mut document = document_with(&[]);
        document.components.callbacks.insert(
            "OnData".to_owned(),
            RefOr::Item(Callback::new().with(key, notify)),
        );

        let emitted = serde_json::to_value(&document).expect("serializable");
        let parsed: Document = serde_json::from_value(emitted).expect("parses");
        let found = errors(&document);
        assert_eq!(
            !found.is_empty(),
            parsed != document,
            "`{key}`: refused with {found:?}, read back as {:?}",
            parsed.components.callbacks
        );
    }
}

/// The refusal sits at the offending key, inline on an operation as well as in
/// `components`, and whether that key holds a Path Item or a `$ref` to one:
/// either is read back as an extension.
#[test]
fn a_callback_keyed_as_an_extension_is_refused_at_its_key() {
    use crate::model::{
        callback::Callback,
        reference::{Ref, RefOr},
    };

    let mut callback = Callback::new();
    callback.items.insert(
        "x-hook".to_owned(),
        RefOr::Ref(Ref::new("#/components/pathItems/Notify")),
    );
    let mut owner = Operation::new("subscribe").with_responses(ok_responses());
    owner
        .callbacks
        .insert("onData".to_owned(), RefOr::Item(callback));
    let document = document_with(&[(
        "/subscriptions",
        PathItem::new().with_operation(Method::Post, owner),
    )]);

    let found: Vec<(String, SpecError)> = Validator::new(SpecVersion::V3_1)
        .validate(&document)
        .into_iter()
        .filter(|violation| violation.severity == super::Severity::Error)
        .map(|violation| (violation.location, violation.error))
        .collect();
    assert_eq!(
        found,
        vec![(
            "#/paths/~1subscriptions/post/callbacks/onData/x-hook".to_owned(),
            SpecError::CallbackKeyedAsExtension {
                expression: "x-hook".to_owned()
            },
        )]
    );
}

/// A response key is refused exactly when the model would not read it back.
///
/// `Responses::with` takes any `u16` and `responses` is a public map, but the
/// reader admits only `[1-5]XX` and three digits beginning `1`-`5`, so `600`
/// and `42` validated cleanly and then failed to parse. `default` and `x-a`
/// parse, but back as the fallback response and an extension, not as the key
/// that was written.
#[test]
fn a_response_key_is_refused_exactly_when_it_does_not_round_trip() {
    for key in [
        "100", "200", "599", "2XX", "600", "42", "099", "2xx", "default", "x-a",
    ] {
        let mut responses = Responses::new();
        responses
            .responses
            .insert(key.to_owned(), crate::RefOr::Item(Response::new("ok")));
        let document = document_with(&[(
            "/users",
            PathItem::new().with_operation(
                Method::Get,
                Operation::new("listUsers").with_responses(responses),
            ),
        )]);

        let reads_back = serde_json::to_value(&document)
            .ok()
            .and_then(|emitted| serde_json::from_value::<Document>(emitted).ok())
            .is_some_and(|parsed| parsed == document);
        let found = errors(&document);
        assert_eq!(
            !found.is_empty(),
            !reads_back,
            "`{key}`: refused with {found:?}, read back: {reads_back}"
        );
    }
}

/// The refusal sits at the offending key, for a code `with` accepts as a `u16`
/// and for a string key carrying a `/`, which the pointer escapes.
#[test]
fn an_unreadable_response_key_is_refused_at_its_key() {
    for (key, token) in [("600", "600"), ("42", "42"), ("2/X", "2~1X")] {
        let mut responses = Responses::new();
        responses
            .responses
            .insert(key.to_owned(), crate::RefOr::Item(Response::new("ok")));
        let document = document_with(&[(
            "/users",
            PathItem::new().with_operation(
                Method::Get,
                Operation::new("listUsers").with_responses(responses),
            ),
        )]);

        let found: Vec<(String, SpecError)> = Validator::new(SpecVersion::V3_1)
            .validate(&document)
            .into_iter()
            .filter(|violation| violation.severity == super::Severity::Error)
            .map(|violation| (violation.location, violation.error))
            .collect();
        assert_eq!(
            found,
            vec![(
                format!("#/paths/~1users/get/responses/{token}"),
                SpecError::InvalidResponseKey {
                    key: key.to_owned()
                },
            )]
        );
    }
}

/// An operation with no responses is reported wherever it is written.
///
/// The companion to the case above, and a different rule on purpose: it shows
/// the walk carries *every* operation-level rule rather than the one that
/// happens to be keyed on a document-wide map.
#[test]
fn a_responseless_operation_outside_paths_is_reported() {
    let mut document = document_with(&[(
        "/users",
        PathItem::new().with_operation(
            Method::Get,
            Operation::new("listUsers").with_responses(ok_responses()),
        ),
    )]);
    document.webhooks.insert(
        "orderPlaced".to_owned(),
        PathItem::new().with_operation(Method::Post, Operation::new("onOrderPlaced")),
    );

    let found = errors(&document);
    assert!(
        found
            .iter()
            .any(|error| matches!(error, SpecError::NoResponses)),
        "got {found:?}"
    );
}

#[test]
fn paths_differing_only_in_variable_name_are_the_same_path() {
    let make = |id: &str| {
        PathItem::new().with_operation(
            Method::Get,
            Operation::new(format!("get{id}"))
                .with_parameter(Parameter::path(id, Schema::of_type(SchemaType::String)))
                .with_responses(ok_responses()),
        )
    };

    let found = errors(&document_with(&[
        ("/pets/{petId}", make("petId")),
        ("/pets/{name}", make("name")),
    ]));
    assert!(
        found
            .iter()
            .any(|e| matches!(e, SpecError::DuplicatePathTemplate { .. })),
        "expected a duplicate path violation, got {found:?}"
    );
}

#[test]
fn a_template_variable_needs_a_matching_path_parameter() {
    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("getUser").with_responses(ok_responses()),
    );
    let found = errors(&document_with(&[("/users/{id}", item)]));
    assert!(matches!(
        found.as_slice(),
        [SpecError::UndeclaredPathVariable { name }] if name == "id"
    ));
}

#[test]
fn a_path_parameter_needs_a_matching_template_variable() {
    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("listUsers")
            .with_parameter(Parameter::path("id", Schema::of_type(SchemaType::String)))
            .with_responses(ok_responses()),
    );
    let found = errors(&document_with(&[("/users", item)]));
    assert!(matches!(
        found.as_slice(),
        [SpecError::UnusedPathParameter { name }] if name == "id"
    ));
}

#[test]
fn path_parameters_hoisted_onto_the_path_item_satisfy_the_template() {
    let mut item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("getUser").with_responses(ok_responses()),
    );
    item.parameters.push(crate::RefOr::Item(Parameter::path(
        "id",
        Schema::of_type(SchemaType::String),
    )));

    assert!(errors(&document_with(&[("/users/{id}", item)])).is_empty());
}

/// `document_with`, plus one reusable parameter under `#/components/parameters`.
fn document_with_component_parameter(
    paths: &[(&str, PathItem)],
    name: &str,
    parameter: crate::RefOr<Parameter>,
) -> Document {
    let mut document = document_with(paths);
    document
        .components
        .parameters
        .insert(name.to_owned(), parameter);
    document
}

fn get_with_ref(location: &str) -> PathItem {
    let mut operation = Operation::new("getUser").with_responses(ok_responses());
    operation
        .parameters
        .push(crate::RefOr::Ref(crate::model::reference::Ref::new(
            location,
        )));
    PathItem::new().with_operation(Method::Get, operation)
}

/// A Reference Object stands in for the Parameter Object it names, so a `$ref`
/// into `#/components/parameters` declares the variable as an inline parameter
/// would.
#[test]
fn a_path_parameter_supplied_by_ref_satisfies_the_template() {
    let document = document_with_component_parameter(
        &[("/users/{id}", get_with_ref("#/components/parameters/Id"))],
        "Id",
        crate::RefOr::Item(Parameter::path("id", Schema::of_type(SchemaType::String))),
    );
    assert!(errors(&document).is_empty(), "got {:?}", errors(&document));
}

/// A component may itself be a reference to another component.
#[test]
fn a_chain_of_component_refs_is_followed_to_the_parameter() {
    let mut document = document_with_component_parameter(
        &[("/users/{id}", get_with_ref("#/components/parameters/Id"))],
        "Id",
        crate::RefOr::Ref(crate::model::reference::Ref::parameter("UserId")),
    );
    document.components.parameters.insert(
        "UserId".to_owned(),
        crate::RefOr::Item(Parameter::path("id", Schema::of_type(SchemaType::String))),
    );
    assert!(errors(&document).is_empty(), "got {:?}", errors(&document));
}

/// The correspondence is two-sided: a referenced path parameter with no
/// variable to fill is unused, as an inline one is.
#[test]
fn a_path_parameter_supplied_by_ref_needs_a_matching_template_variable() {
    let document = document_with_component_parameter(
        &[("/users", get_with_ref("#/components/parameters/Id"))],
        "Id",
        crate::RefOr::Item(Parameter::path("id", Schema::of_type(SchemaType::String))),
    );
    assert!(matches!(
        errors(&document).as_slice(),
        [SpecError::UnusedPathParameter { name }] if name == "id"
    ));
}

/// A reference this document cannot resolve may name the variable, so the
/// variable is not reported undeclared on a guess: an external document, and a
/// pointer that is not a component name, are both outside what a validator of
/// one document can read.
#[test]
fn a_ref_outside_the_components_does_not_fabricate_an_undeclared_variable() {
    for location in [
        "common.yaml#/components/parameters/Id",
        "#/paths/~1accounts~1{id}/get/parameters/0",
    ] {
        let found = errors(&document_with(&[("/users/{id}", get_with_ref(location))]));
        assert!(found.is_empty(), "{location}: got {found:?}");
    }
}

/// A local reference to a component that does not exist declares nothing, so
/// the variable it was meant to declare is still undeclared.
#[test]
fn a_dangling_component_ref_declares_nothing() {
    let found = errors(&document_with(&[(
        "/users/{id}",
        get_with_ref("#/components/parameters/Id"),
    )]));
    assert!(matches!(
        found.as_slice(),
        [SpecError::UndeclaredPathVariable { name }] if name == "id"
    ));
}

/// Components that reference each other in a cycle stand for no parameter, and
/// following them terminates.
#[test]
fn a_cycle_of_component_refs_declares_nothing() {
    let mut document = document_with_component_parameter(
        &[("/users/{id}", get_with_ref("#/components/parameters/A"))],
        "A",
        crate::RefOr::Ref(crate::model::reference::Ref::parameter("B")),
    );
    document.components.parameters.insert(
        "B".to_owned(),
        crate::RefOr::Ref(crate::model::reference::Ref::parameter("A")),
    );
    assert!(matches!(
        errors(&document).as_slice(),
        [SpecError::UndeclaredPathVariable { name }] if name == "id"
    ));
}

/// Each error a 3.1 validation raises, with where it was raised.
fn located_errors(document: &Document) -> Vec<(String, SpecError)> {
    Validator::new(SpecVersion::V3_1)
        .validate(document)
        .into_iter()
        .filter(|v| v.severity == super::Severity::Error)
        .map(|v| (v.location, v.error))
        .collect()
}

/// A reusable parameter is held to the rules an inline one is, and reported
/// where it is defined: once, however many operations reference it.
#[test]
fn a_reusable_path_parameter_must_be_required_once_at_its_definition() {
    use crate::model::reference::{Ref, RefOr};

    let with_ref = |operation_id| {
        let mut operation = Operation::new(operation_id).with_responses(ok_responses());
        operation.parameters.push(RefOr::Ref(Ref::parameter("Id")));
        operation
    };
    let item = PathItem::new()
        .with_operation(Method::Get, with_ref("getUser"))
        .with_operation(Method::Delete, with_ref("deleteUser"));
    let mut parameter = Parameter::path("id", Schema::of_type(SchemaType::String));
    parameter.required = Some(false);
    let document =
        document_with_component_parameter(&[("/users/{id}", item)], "Id", RefOr::Item(parameter));

    assert_eq!(
        located_errors(&document),
        vec![(
            "#/components/parameters/Id".to_owned(),
            SpecError::PathParameterNotRequired {
                name: "id".to_owned()
            },
        )]
    );
}

/// The definition is checked whether or not anything references it.
#[test]
fn a_reusable_header_parameter_the_spec_ignores_is_reported() {
    let document = document_with_component_parameter(
        &[],
        "Accept",
        crate::RefOr::Item(Parameter::header(
            "Accept",
            Schema::of_type(SchemaType::String),
        )),
    );

    assert_eq!(
        located_errors(&document),
        vec![(
            "#/components/parameters/Accept".to_owned(),
            SpecError::IgnoredHeaderParameter {
                name: "Accept".to_owned()
            },
        )]
    );
}

/// The violation names the style as a description spells it, so every
/// multi-word style is a row.
#[test]
fn a_reusable_parameters_style_is_checked_against_its_location() {
    for (style, spelled) in [
        (Style::DeepObject, "deepObject"),
        (Style::SpaceDelimited, "spaceDelimited"),
        (Style::PipeDelimited, "pipeDelimited"),
    ] {
        let document = document_with_component_parameter(
            &[],
            "Trace",
            crate::RefOr::Item(
                Parameter::header("X-Trace", Schema::of_type(SchemaType::String))
                    .with_style(style, false),
            ),
        );

        assert_eq!(
            located_errors(&document),
            vec![(
                "#/components/parameters/Trace".to_owned(),
                SpecError::IllegalStyle {
                    style: spelled.to_owned(),
                    location: "header".to_owned(),
                },
            )],
            "{spelled}"
        );
    }
}

/// A component's name is a pointer token, escaped as one. The extension-name
/// rule warns of a reserved prefix rather than erring, so every severity is
/// read here.
#[test]
fn a_reusable_parameters_extensions_are_checked_at_its_escaped_pointer() {
    let mut parameter = Parameter::query("page", Schema::of_type(SchemaType::Integer));
    parameter.extensions.insert("x-oai-reserved", true);
    let document = document_with_component_parameter(&[], "a/b", crate::RefOr::Item(parameter));

    let found = Validator::new(SpecVersion::V3_1).validate(&document);
    assert!(
        found.iter().any(|violation| {
            violation.location == "#/components/parameters/a~1b"
                && matches!(violation.error, SpecError::InvalidExtensionName { .. })
        }),
        "got {found:?}"
    );
}

#[test]
fn a_path_parameter_must_be_required() {
    let mut parameter = Parameter::path("id", Schema::of_type(SchemaType::String));
    parameter.required = Some(false);

    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("getUser")
            .with_parameter(parameter)
            .with_responses(ok_responses()),
    );

    let found = errors(&document_with(&[("/users/{id}", item)]));
    assert!(
        found
            .iter()
            .any(|e| matches!(e, SpecError::PathParameterNotRequired { .. })),
        "got {found:?}"
    );
}

#[test]
fn duplicate_name_and_location_pairs_are_rejected() {
    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("listUsers")
            .with_parameter(Parameter::query(
                "page",
                Schema::of_type(SchemaType::Integer),
            ))
            .with_parameter(Parameter::query(
                "page",
                Schema::of_type(SchemaType::Integer),
            ))
            .with_responses(ok_responses()),
    );

    let found = errors(&document_with(&[("/users", item)]));
    assert!(matches!(
        found.as_slice(),
        [SpecError::DuplicateParameter { name, .. }] if name == "page"
    ));
}

/// The list is unique by what each entry *is*, so a duplicate that arrives by
/// `$ref` is a duplicate.
#[test]
fn a_duplicate_arriving_by_ref_is_rejected() {
    let mut operation = Operation::new("listUsers")
        .with_parameter(Parameter::query(
            "page",
            Schema::of_type(SchemaType::Integer),
        ))
        .with_responses(ok_responses());
    operation
        .parameters
        .push(crate::RefOr::Ref(crate::model::reference::Ref::parameter(
            "Page",
        )));
    let document = document_with_component_parameter(
        &[(
            "/users",
            PathItem::new().with_operation(Method::Get, operation),
        )],
        "Page",
        crate::RefOr::Item(Parameter::query(
            "page",
            Schema::of_type(SchemaType::Integer),
        )),
    );

    assert!(matches!(
        errors(&document).as_slice(),
        [SpecError::DuplicateParameter { name, .. }] if name == "page"
    ));
}

/// Two spellings of one header field are one parameter, not two.
///
/// RFC 9110 section 5.1 makes field names case-insensitive, and
/// [`is_ignored_header_parameter`](crate::model::parameter::header::is_ignored_header_parameter)
/// already folds case "matching HTTP header semantics" -- so a uniqueness key
/// that does not fold makes this rule disagree with the one beside it, and lets
/// a document naming one field twice through.
#[test]
fn two_cases_of_one_header_name_are_one_parameter() {
    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("listUsers")
            .with_parameter(Parameter::header(
                "Accept-Language",
                Schema::of_type(SchemaType::String),
            ))
            .with_parameter(Parameter::header(
                "accept-language",
                Schema::of_type(SchemaType::String),
            ))
            .with_responses(ok_responses()),
    );

    let found = errors(&document_with(&[("/users", item)]));
    assert!(
        found
            .iter()
            .any(|error| matches!(error, SpecError::DuplicateParameter { .. })),
        "got {found:?}"
    );
}

/// And a location where names *are* case-sensitive keeps both.
#[test]
fn two_cases_of_one_query_name_are_two_parameters() {
    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("listUsers")
            .with_parameter(Parameter::query(
                "Page",
                Schema::of_type(SchemaType::Integer),
            ))
            .with_parameter(Parameter::query(
                "page",
                Schema::of_type(SchemaType::Integer),
            ))
            .with_responses(ok_responses()),
    );

    assert!(errors(&document_with(&[("/users", item)])).is_empty());
}

#[test]
fn the_same_name_in_two_locations_is_fine() {
    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("listUsers")
            .with_parameter(Parameter::query(
                "token",
                Schema::of_type(SchemaType::String),
            ))
            .with_parameter(Parameter::cookie(
                "token",
                Schema::of_type(SchemaType::String),
            ))
            .with_responses(ok_responses()),
    );
    assert!(errors(&document_with(&[("/users", item)])).is_empty());
}

#[test]
fn headers_the_spec_ignores_may_not_be_declared_as_parameters() {
    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("listUsers")
            .with_parameter(Parameter::header(
                "Authorization",
                Schema::of_type(SchemaType::String),
            ))
            .with_responses(ok_responses()),
    );

    let found = errors(&document_with(&[("/users", item)]));
    assert!(matches!(
        found.as_slice(),
        [SpecError::IgnoredHeaderParameter { name }] if name == "Authorization"
    ));
}

#[test]
fn styles_are_checked_against_the_parameter_location() {
    let parameter = Parameter::header("X-Trace", Schema::of_type(SchemaType::String))
        .with_style(Style::DeepObject, false);

    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("listUsers")
            .with_parameter(parameter)
            .with_responses(ok_responses()),
    );

    let found = errors(&document_with(&[("/users", item)]));
    assert!(matches!(found.as_slice(), [SpecError::IllegalStyle { .. }]));
}

#[test]
fn a_response_header_named_content_type_is_reported() {
    use crate::model::parameter::header::Header;

    // The specification says a response header named `Content-Type` shall be
    // ignored, which makes declaring one a silent lie the same way an ignored
    // header parameter is.
    let response = Response::new("ok").with_header(
        "Content-Type",
        Header::new(Schema::of_type(SchemaType::String)),
    );

    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("listUsers").with_responses(Responses::new().with(200, response)),
    );

    let found = errors(&document_with(&[("/users", item)]));
    assert_eq!(
        found.len(),
        1,
        "a `Content-Type` response header should be reported exactly once, got {found:?}"
    );
    assert!(
        found[0].to_string().contains("Content-Type"),
        "the violation should name the header, got: {}",
        found[0]
    );
}

#[test]
fn an_encoding_header_named_content_type_is_reported() {
    use crate::model::{
        body::{RequestBody, encoding::Encoding, media_type::MediaType},
        parameter::header::Header,
    };

    // The same rule reaches an encoding's headers, and header names are
    // case-insensitive, so the spelling must not decide whether it is caught.
    let encoding = Encoding::new("text/plain").with_header(
        "content-type",
        Header::new(Schema::of_type(SchemaType::String)),
    );

    let body = RequestBody::new(
        "multipart/form-data",
        MediaType::new(object_with(&["part"])).with_encoding("part", encoding),
    );

    let item = PathItem::new().with_operation(
        Method::Post,
        Operation::new("upload")
            .with_request_body(body)
            .with_responses(ok_responses()),
    );

    let found = errors(&document_with(&[("/uploads", item)]));
    assert_eq!(
        found.len(),
        1,
        "a `content-type` encoding header should be reported exactly once, got {found:?}"
    );
    assert!(
        found[0].to_string().contains("content-type"),
        "the violation should name the header, got: {}",
        found[0]
    );
}

/// A response whose only defect is a `Content-Type` header the specification
/// says shall be ignored.
fn response_with_ignored_header() -> Response {
    use crate::model::parameter::header::Header;

    Response::new("ok").with_header(
        "Content-Type",
        Header::new(Schema::of_type(SchemaType::String)),
    )
}

/// The content rules do not depend on where the object sits, so the `default`
/// response is held to them as a keyed one is.
#[test]
fn the_default_responses_headers_are_checked() {
    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("listUsers")
            .with_responses(ok_responses().with_default(response_with_ignored_header())),
    );

    assert_eq!(
        located_errors(&document_with(&[("/users", item)])),
        vec![(
            "#/paths/~1users/get/responses/default/headers".to_owned(),
            SpecError::IgnoredHeader {
                name: "Content-Type".to_owned()
            },
        )]
    );
}

/// A reusable response is checked once, where it is defined, whether or not
/// anything references it.
#[test]
fn a_reusable_responses_headers_are_checked_at_its_definition() {
    let mut document = document_with(&[]);
    document.components.responses.insert(
        "Typed".to_owned(),
        crate::RefOr::Item(response_with_ignored_header()),
    );

    assert_eq!(
        located_errors(&document),
        vec![(
            "#/components/responses/Typed/headers".to_owned(),
            SpecError::IgnoredHeader {
                name: "Content-Type".to_owned()
            },
        )]
    );
}

/// A media type declaring both `encoding` and `prefixEncoding`, and nothing
/// else wrong: the key names a property, and `itemSchema` is the array.
#[cfg(feature = "openapi32")]
fn conflicting_media_type() -> crate::model::body::media_type::MediaType {
    use crate::model::body::{encoding::Encoding, media_type::MediaType};

    let mut content = MediaType::new(object_with(&["part"]));
    content.item_schema = Some(Schema::of_type(SchemaType::String));
    content
        .encoding
        .insert("part".to_owned(), Encoding::new("text/plain"));
    content.prefix_encoding = Some(vec![Encoding::new("text/plain")]);
    content
}

/// An object schema declaring `properties`, each a string.
fn object_with(properties: &[&str]) -> Schema {
    let mut schema = crate::model::schema::object::SchemaObject {
        ty: Some(crate::model::schema::types::TypeSet::One(
            SchemaType::Object,
        )),
        ..Default::default()
    };
    for property in properties {
        schema
            .properties
            .insert((*property).to_owned(), Schema::of_type(SchemaType::String));
    }
    Schema::Object(Box::new(schema))
}

/// A document whose one operation uploads `content` as `media_type`.
fn document_with_upload(
    media_type: &str,
    content: crate::model::body::media_type::MediaType,
) -> Document {
    use crate::model::body::RequestBody;

    let item = PathItem::new().with_operation(
        Method::Post,
        Operation::new("upload")
            .with_request_body(RequestBody::new(media_type, content))
            .with_responses(ok_responses()),
    );
    document_with(&[("/uploads", item)])
}

/// The `content` of [`document_with_upload`]'s request body.
const UPLOAD: &str = "#/paths/~1uploads/post/requestBody/content";

/// Each `encoding` key names a property (`references/3.1.2.md:1551`,
/// `references/3.2.0.md:1368`), reported at the entry naming none.
#[test]
fn an_encoding_key_naming_no_property_is_an_error() {
    use crate::model::body::{encoding::Encoding, media_type::MediaType};

    let content = MediaType::new(object_with(&["file"]))
        .with_encoding("file", Encoding::new("image/png"))
        .with_encoding("fiel", Encoding::new("text/plain"));

    assert_eq!(
        located_errors(&document_with_upload("multipart/form-data", content)),
        vec![(
            format!("{UPLOAD}/multipart~1form-data/encoding/fiel"),
            SpecError::UnknownEncodingProperty {
                property: "fiel".to_owned()
            },
        )]
    );
}

/// With no schema there is no property for any key to name.
#[test]
fn an_encoding_key_without_a_schema_is_an_error() {
    use crate::model::body::{encoding::Encoding, media_type::MediaType};

    let content = MediaType::default().with_encoding("file", Encoding::new("image/png"));

    assert_eq!(
        located_errors(&document_with_upload("multipart/form-data", content)),
        vec![(
            format!("{UPLOAD}/multipart~1form-data/encoding/file"),
            SpecError::UnknownEncodingProperty {
                property: "file".to_owned()
            },
        )]
    );
}

/// A property is found through a component reference and `allOf`, the
/// shapes Kynos's own derived forms take; one an unread keyword could
/// declare is not judged, so no valid document is refused.
#[test]
fn an_encoding_key_is_judged_only_where_every_declaring_keyword_was_read() {
    use crate::model::{
        body::{encoding::Encoding, media_type::MediaType},
        schema::object::SchemaObject,
    };

    let composed = Schema::Object(Box::new(SchemaObject {
        all_of: Some(vec![Schema::component("Upload"), object_with(&["note"])]),
        ..Default::default()
    }));
    let alternatives = Schema::Object(Box::new(SchemaObject {
        one_of: Some(vec![object_with(&["a"]), object_with(&["b"])]),
        ..Default::default()
    }));
    let cyclic = Schema::component("Loop");

    let mut document = document_with(&[]);
    for (path, schema, key) in [
        ("/composed", composed.clone(), "file"),
        ("/composed-own", composed, "note"),
        ("/alternatives", alternatives, "c"),
        (
            "/elsewhere",
            Schema::reference("other.json#/Upload"),
            "file",
        ),
        ("/cyclic", cyclic, "file"),
        ("/unconstrained", Schema::any(), "file"),
    ] {
        document.paths.insert(
            &PathTemplate::parse(path).expect("a legal template"),
            PathItem::new().with_operation(
                Method::Post,
                Operation::new(path.trim_start_matches('/'))
                    .with_request_body(crate::model::body::RequestBody::new(
                        "multipart/form-data",
                        MediaType::new(schema).with_encoding(key, Encoding::new("text/plain")),
                    ))
                    .with_responses(ok_responses()),
            ),
        );
    }
    document
        .components
        .schemas
        .insert("Upload".to_owned(), object_with(&["file"]));
    // A reference cycle is walked once, and still finds what it declares.
    document.components.schemas.insert(
        "Loop".to_owned(),
        Schema::Object(Box::new(SchemaObject {
            all_of: Some(vec![Schema::component("Loop"), object_with(&["file"])]),
            ..Default::default()
        })),
    );

    assert_eq!(located_errors(&document), vec![]);
}

/// `prefixEncoding` and `itemEncoding` need `itemSchema` or an array
/// `schema` (`references/3.2.0.md:1386`), reported once at the media type,
/// including where `array` is absent from several types.
#[cfg(feature = "openapi32")]
#[test]
fn a_positional_encoding_without_an_array_is_an_error() {
    use crate::model::{
        body::{encoding::Encoding, media_type::MediaType},
        schema::object::SchemaObject,
    };

    let positional = |schema: Option<Schema>| {
        let mut content = MediaType::default();
        content.schema = schema;
        content.prefix_encoding = Some(vec![Encoding::new("text/plain")]);
        content.item_encoding = Some(Box::new(Encoding::new("text/plain")));
        content
    };
    let through_all_of = Schema::Object(Box::new(SchemaObject {
        all_of: Some(vec![Schema::component("Text")]),
        ..Default::default()
    }));

    let mut document = document_with(&[]);
    for (path, content) in [
        ("/absent", positional(None)),
        ("/object", positional(Some(object_with(&[])))),
        ("/referenced", positional(Some(through_all_of))),
        (
            "/nullable",
            positional(Some(Schema::nullable(SchemaType::String))),
        ),
    ] {
        document.paths.insert(
            &PathTemplate::parse(path).expect("a legal template"),
            PathItem::new().with_operation(
                Method::Post,
                Operation::new(path.trim_start_matches('/'))
                    .with_request_body(crate::model::body::RequestBody::new(
                        "multipart/mixed",
                        content,
                    ))
                    .with_responses(ok_responses()),
            ),
        );
    }
    document
        .components
        .schemas
        .insert("Text".to_owned(), Schema::of_type(SchemaType::String));

    let at = |path: &str| {
        (
            format!("#/paths/~1{path}/post/requestBody/content/multipart~1mixed"),
            SpecError::PositionalEncodingWithoutArray,
        )
    };
    assert_eq!(
        located_errors_3_2(&document),
        vec![at("absent"), at("object"), at("referenced"), at("nullable")]
    );
}

/// An array `schema`, `itemSchema` (the `multipart/byteranges` shape Kynos
/// emits for a ranged response), or a schema that may be an array, untyped or
/// with `array` among several types, satisfies the positional encodings.
#[cfg(feature = "openapi32")]
#[test]
fn a_positional_encoding_beside_a_possible_array_is_clean() {
    use crate::model::body::{encoding::Encoding, media_type::MediaType};

    let mut array = MediaType::new(Schema::of_type(SchemaType::Array));
    array.prefix_encoding = Some(vec![Encoding::new("text/plain")]);
    let mut byteranges = MediaType::sequential(Schema::Object(Box::default()));
    byteranges.item_encoding = Some(Box::new(Encoding::new("text/plain")));
    let mut untyped = MediaType::new(Schema::component("Parts"));
    untyped.item_encoding = Some(Box::new(Encoding::new("text/plain")));
    let mut nullable = MediaType::new(Schema::nullable(SchemaType::Array));
    nullable.prefix_encoding = Some(vec![Encoding::new("text/plain")]);
    nullable.item_encoding = Some(Box::new(Encoding::new("text/plain")));

    let mut document = document_with(&[]);
    for (name, content) in [
        ("Array", array),
        ("Byteranges", byteranges),
        ("Nullable", nullable),
        ("Untyped", untyped),
    ] {
        document
            .components
            .media_types
            .insert(name.to_owned(), crate::RefOr::Item(content));
    }
    document
        .components
        .schemas
        .insert("Parts".to_owned(), Schema::Object(Box::default()));

    assert_eq!(located_errors_3_2(&document), vec![]);
}

/// A nested Encoding Object applies its fields "in the same manner" as a
/// media type's (`references/3.2.0.md:1711-1713`), so its own `encoding`
/// excludes its positional ones and its headers drop `Content-Type`, at
/// every depth and under every positional field.
#[cfg(feature = "openapi32")]
#[test]
fn a_nested_encoding_is_held_to_the_media_type_rules() {
    use crate::model::{
        body::{encoding::Encoding, media_type::MediaType},
        parameter::header::Header,
    };

    let typed = || {
        Encoding::new("text/plain").with_header(
            "Content-Type",
            Header::new(Schema::of_type(SchemaType::String)),
        )
    };
    let mut conflicting = Encoding::new("multipart/mixed");
    conflicting
        .encoding
        .insert("inner".to_owned(), Encoding::new("text/plain"));
    conflicting.item_encoding = Some(Box::new(typed()));

    let mut form = MediaType::new(object_with(&["part"]));
    form.encoding.insert("part".to_owned(), conflicting);
    let mut parts = MediaType::new(Schema::of_type(SchemaType::Array));
    parts.prefix_encoding = Some(vec![typed()]);
    parts.item_encoding = Some(Box::new(typed()));

    let mut document = document_with(&[]);
    document
        .components
        .media_types
        .insert("Form".to_owned(), crate::RefOr::Item(form));
    document
        .components
        .media_types
        .insert("Parts".to_owned(), crate::RefOr::Item(parts));

    let ignored = |location: &str| {
        (
            location.to_owned(),
            SpecError::IgnoredHeader {
                name: "Content-Type".to_owned(),
            },
        )
    };
    assert_eq!(
        located_errors_3_2(&document),
        vec![
            (
                "#/components/mediaTypes/Form/encoding/part".to_owned(),
                SpecError::ConflictingEncoding,
            ),
            ignored("#/components/mediaTypes/Form/encoding/part/itemEncoding/headers"),
            ignored("#/components/mediaTypes/Parts/prefixEncoding/0/headers"),
            ignored("#/components/mediaTypes/Parts/itemEncoding/headers"),
        ]
    );
}

/// Each error a 3.2 validation raises, with where it was raised.
#[cfg(feature = "openapi32")]
fn located_errors_3_2(document: &Document) -> Vec<(String, SpecError)> {
    Validator::new(SpecVersion::V3_2)
        .validate(document)
        .into_iter()
        .filter(|v| v.severity == super::Severity::Error)
        .map(|v| (v.location, v.error))
        .collect()
}

/// A key without the `x-` prefix is no field either version defines and no
/// extension (`references/3.2.0.md:4747`), so 3.2 refuses it as 3.1 does: at
/// the object holding it, by the name it was written under, and once.
#[cfg(feature = "openapi32")]
#[test]
fn an_unrecognised_field_is_an_error_under_3_2() {
    let mut response = Response::new("ok");
    response.extensions.insert("descripton", "typo");
    let mut operation =
        Operation::new("getUser").with_responses(Responses::new().with(200, response));
    operation.extensions.insert("opertionId", "typo");
    operation.extensions.insert("odd/key~", true);
    let document = document_with(&[(
        "/users",
        PathItem::new().with_operation(Method::Get, operation),
    )]);

    let unrecognised = |location: &str, name: &str| Violation {
        location: location.to_owned(),
        severity: super::Severity::Error,
        error: SpecError::InvalidExtensionName {
            name: name.to_owned(),
        },
    };
    assert_eq!(
        Validator::new(SpecVersion::V3_2).validate(&document),
        vec![
            unrecognised("#/paths/~1users/get", "opertionId"),
            unrecognised("#/paths/~1users/get", "odd/key~"),
            unrecognised("#/paths/~1users/get/responses/200", "descripton"),
        ]
    );
}

/// An `x-oai-` or `x-oas-` name is an extension the OpenAPI Initiative
/// reserves: weaker than it could be, not unreadable, so it stays a warning
/// under 3.2.
#[cfg(feature = "openapi32")]
#[test]
fn a_reserved_extension_name_stays_a_warning_under_3_2() {
    let mut operation = Operation::new("getUser").with_responses(ok_responses());
    operation.extensions.insert("x-oai-thing", true);
    let document = document_with(&[(
        "/users",
        PathItem::new().with_operation(Method::Get, operation),
    )]);

    assert_eq!(
        Validator::new(SpecVersion::V3_2).validate(&document),
        vec![Violation {
            location: "#/paths/~1users/get".to_owned(),
            severity: super::Severity::Warning,
            error: SpecError::InvalidExtensionName {
                name: "x-oai-thing".to_owned(),
            },
        }]
    );
}

#[cfg(feature = "openapi32")]
#[test]
fn a_reusable_request_bodys_media_types_are_checked_at_its_definition() {
    use crate::model::body::RequestBody;

    let mut document = document_with(&[]);
    document.components.request_bodies.insert(
        "Upload".to_owned(),
        crate::RefOr::Item(RequestBody::new(
            "multipart/mixed",
            conflicting_media_type(),
        )),
    );

    assert_eq!(
        located_errors_3_2(&document),
        vec![(
            "#/components/requestBodies/Upload/content/multipart~1mixed".to_owned(),
            SpecError::ConflictingEncoding,
        )]
    );
}

#[cfg(feature = "openapi32")]
#[test]
fn a_reusable_media_type_is_checked_at_its_definition() {
    let mut document = document_with(&[]);
    document.components.media_types.insert(
        "Parts".to_owned(),
        crate::RefOr::Item(conflicting_media_type()),
    );

    assert_eq!(
        located_errors_3_2(&document),
        vec![(
            "#/components/mediaTypes/Parts".to_owned(),
            SpecError::ConflictingEncoding,
        )]
    );
}

/// `encoding` "MUST NOT be present if `prefixEncoding` or `itemEncoding` are
/// present" (`references/3.2.0.md:1265-1267`), and nothing else is excluded:
/// each row sets `encoding`, `prefixEncoding` and `itemEncoding` or not, on a
/// media type otherwise clean, and states whether the exclusion is reported.
#[cfg(feature = "openapi32")]
#[test]
fn encoding_excludes_each_positional_field_and_nothing_else() {
    use crate::model::body::encoding::Encoding;

    for (encoding, prefix, item, conflicts) in [
        (true, true, false, true),
        (true, false, true, true),
        (false, true, true, false),
        (true, false, false, false),
    ] {
        let mut content = conflicting_media_type();
        if !encoding {
            content.encoding.clear();
        }
        if !prefix {
            content.prefix_encoding = None;
        }
        if item {
            content.item_encoding = Some(Box::new(Encoding::new("text/plain")));
        }
        let mut document = document_with(&[]);
        document
            .components
            .media_types
            .insert("Parts".to_owned(), crate::RefOr::Item(content));

        let expected = if conflicts {
            vec![(
                "#/components/mediaTypes/Parts".to_owned(),
                SpecError::ConflictingEncoding,
            )]
        } else {
            vec![]
        };
        assert_eq!(
            located_errors_3_2(&document),
            expected,
            "encoding {encoding}, prefixEncoding {prefix}, itemEncoding {item}"
        );
    }
}

/// An `additionalOperations` key is refused exactly when its method has a
/// fixed field of its own.
///
/// `references/3.2.0.md:604`: the map "MUST NOT contain any entry for the
/// methods that can be defined by other fixed fields". Every method in
/// [`Method::all`] is one; `PROPFIND` has no field, and `post` is not `POST`,
/// because a method token is case-sensitive (RFC 9110 §9.1).
#[cfg(feature = "openapi32")]
#[test]
fn an_additional_operation_is_refused_exactly_when_its_method_has_a_field() {
    let fixed = Method::all()
        .iter()
        .map(|method| (method.as_wire_str(), true));
    let unfixed = [("PROPFIND", false), ("post", false)];

    for (key, refused) in fixed.chain(unfixed) {
        let mut item = PathItem::new();
        item.additional_operations.insert(
            key.to_owned(),
            Box::new(Operation::new("op").with_responses(ok_responses())),
        );
        let document = document_with(&[("/x", item)]);
        let at_key = format!("#/paths/~1x/additionalOperations/{key}");

        let located: Vec<String> = located_errors_3_2(&document)
            .into_iter()
            .map(|(location, _)| location)
            .collect();
        assert_eq!(located.contains(&at_key), refused, "`{key}`: {located:?}");
    }
}

/// The refusal names the method, sits at the escaped key, and comes once per
/// offending key, beside the fixed field it duplicates or without it.
#[cfg(feature = "openapi32")]
#[test]
fn a_fixed_method_in_additional_operations_is_reported_at_its_key() {
    let operation = |id: &str| Box::new(Operation::new(id).with_responses(ok_responses()));
    let mut item = PathItem::new().with_operation(Method::Post, *operation("create"));
    item.additional_operations
        .insert("POST".to_owned(), operation("createAgain"));
    item.additional_operations
        .insert("QUERY".to_owned(), operation("search"));
    item.additional_operations
        .insert("PROPFIND".to_owned(), operation("properties"));

    assert_eq!(
        located_errors_3_2(&document_with(&[("/a/b", item)])),
        vec![
            (
                "#/paths/~1a~1b/additionalOperations/POST".to_owned(),
                SpecError::FixedMethodInAdditionalOperations {
                    method: Method::Post
                },
            ),
            (
                "#/paths/~1a~1b/additionalOperations/QUERY".to_owned(),
                SpecError::FixedMethodInAdditionalOperations {
                    method: Method::Query
                },
            ),
        ]
    );
}

/// Every component section's keys are checked, not five of them.
///
/// `references/3.1.2.md:593`: "**All** the fixed fields declared above are
/// objects that MUST use keys that match `^[a-zA-Z0-9\.\-_]+$`". The check
/// covered `schemas`, `responses`, `parameters`, `requestBodies` and
/// `securitySchemes`, so a key in any of the other five — six under 3.2 —
/// passed whatever it was.
///
/// One case per section rather than one representative, because what is under
/// test is the *set* of sections consulted; a representative section is the
/// thing that was already passing.
#[test]
fn every_component_section_checks_its_keys() {
    use crate::model::{
        callback::Callback, example::Example, link::Link, parameter::header::Header,
        paths::item::PathItem, reference::RefOr, schema::Schema,
    };

    const BAD: &str = "not a name";

    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("listUsers").with_responses(ok_responses()),
    );
    let mut document = document_with(&[("/users", item)]);
    let components = &mut document.components;
    components
        .schemas
        .insert(BAD.to_owned(), Schema::of_type(SchemaType::String));
    components
        .responses
        .insert(BAD.to_owned(), RefOr::Item(Response::new("ok")));
    components.parameters.insert(
        BAD.to_owned(),
        RefOr::Item(Parameter::query("q", Schema::of_type(SchemaType::String))),
    );
    components.examples.insert(
        BAD.to_owned(),
        RefOr::Item(Example::new(serde_json::json!("x"))),
    );
    components.request_bodies.insert(
        BAD.to_owned(),
        RefOr::Item(crate::model::body::RequestBody::default()),
    );
    components.headers.insert(
        BAD.to_owned(),
        RefOr::Item(Header::new(Schema::of_type(SchemaType::String))),
    );
    components.security_schemes.insert(
        BAD.to_owned(),
        RefOr::Item(crate::model::security::SecurityScheme::basic()),
    );
    components
        .links
        .insert(BAD.to_owned(), RefOr::Item(Link::to_operation("listUsers")));
    components
        .callbacks
        .insert(BAD.to_owned(), RefOr::Item(Callback::new()));
    components
        .path_items
        .insert(BAD.to_owned(), PathItem::new());
    #[cfg(feature = "openapi32")]
    components.media_types.insert(
        BAD.to_owned(),
        RefOr::Item(crate::model::body::media_type::MediaType::new(
            Schema::of_type(SchemaType::String),
        )),
    );

    let reported: Vec<String> = violations(&document)
        .into_iter()
        .filter(|v| matches!(v.error, SpecError::InvalidComponentName { .. }))
        .map(|v| v.location)
        .collect();

    let expected = [
        "schemas",
        "responses",
        "parameters",
        "examples",
        "requestBodies",
        "headers",
        "securitySchemes",
        "links",
        "callbacks",
        "pathItems",
        #[cfg(feature = "openapi32")]
        "mediaTypes",
    ];

    for section in expected {
        assert!(
            reported
                .iter()
                .any(|location| location.starts_with(&format!("#/components/{section}/"))),
            "`components.{section}` accepted `{BAD}` as a key; reported: {reported:?}"
        );
    }
}

/// A component name is a pointer token: unescaped, `a/b~c` would point two
/// levels below a key `a`, not at the key itself.
#[test]
fn an_invalid_component_name_is_located_at_its_escaped_pointer() {
    let mut document = document_with(&[]);
    document
        .components
        .schemas
        .insert("a/b~c".to_owned(), Schema::of_type(SchemaType::String));

    let located: Vec<String> = violations(&document)
        .into_iter()
        .filter(|v| matches!(v.error, SpecError::InvalidComponentName { .. }))
        .map(|v| v.location)
        .collect();

    assert_eq!(located, ["#/components/schemas/a~1b~0c"]);
}

/// A server variable's name is a pointer token, escaped as one.
#[test]
fn a_server_variable_is_located_at_its_escaped_pointer() {
    use crate::model::server::{Server, ServerVariable};

    let mut document = document_with(&[]);
    document.servers = vec![Server::new("https://example.com").with_variable(
        "a/b~c",
        ServerVariable {
            enumeration: Some(Vec::new()),
            default_value: "eu".to_owned(),
            description: None,
            extensions: crate::model::extensions::Extensions::new(),
        },
    )];

    let located: Vec<String> = violations(&document)
        .into_iter()
        .filter(|v| matches!(v.error, SpecError::EmptyServerVariableEnum { .. }))
        .map(|v| v.location)
        .collect();

    assert_eq!(located, ["#/servers/0/variables/a~1b~0c"]);
}

/// Every Server Object is held to the Server Variable MUSTs, not only the
/// root `servers`.
///
/// `references/3.1.2.md:565-566`: `enum` "MUST NOT be empty" and `default`
/// "MUST exist in the enum's values". A Path Item, an Operation and a Link
/// each carry a Server Object of the same type, and `check_servers` walked
/// `document.servers` alone. One case per scope, each with both violations,
/// each at its escaped pointer.
#[test]
fn a_server_variable_is_checked_at_every_scope() {
    use crate::model::{
        link::Link,
        reference::RefOr,
        server::{Server, ServerVariable},
    };

    let bad_server = || {
        Server::new("https://example.com/{a/b~c}/{d}")
            .with_variable(
                "a/b~c",
                ServerVariable::enumerated("eu", Vec::<String>::new()),
            )
            .with_variable("d", ServerVariable::enumerated("eu", ["us"]))
    };
    let link = || {
        let mut link = Link::to_operation("listUsers");
        link.server = Some(bad_server());
        link
    };
    let operation = || Operation::new("listUsers").with_responses(ok_responses());

    let mut path_item = PathItem::new().with_operation(Method::Get, operation());
    path_item.servers = vec![bad_server()];

    let mut on_operation = operation();
    on_operation.servers = vec![bad_server()];

    let linked = operation().with_responses(
        Responses::new().with(200, Response::new("ok").with_link("next/page", link())),
    );

    let mut component_link = document_with(&[]);
    component_link
        .components
        .links
        .insert("a/b".to_owned(), RefOr::Item(link()));

    let mut component_response = document_with(&[]);
    component_response.components.responses.insert(
        "a/b".to_owned(),
        RefOr::Item(Response::new("ok").with_link("next", link())),
    );

    let cases = [
        (
            document_with(&[("/users", path_item)]),
            "#/paths/~1users/servers/0",
        ),
        (
            document_with(&[(
                "/users",
                PathItem::new().with_operation(Method::Get, on_operation),
            )]),
            "#/paths/~1users/get/servers/0",
        ),
        (
            document_with(&[(
                "/users",
                PathItem::new().with_operation(Method::Get, linked),
            )]),
            "#/paths/~1users/get/responses/200/links/next~1page/server",
        ),
        (component_link, "#/components/links/a~1b/server"),
        (
            component_response,
            "#/components/responses/a~1b/links/next/server",
        ),
    ];

    for (document, server) in cases {
        let located: Vec<(String, &str)> = violations(&document)
            .into_iter()
            .filter_map(|v| match v.error {
                SpecError::EmptyServerVariableEnum { .. } => Some((v.location, "empty")),
                SpecError::ServerVariableDefaultNotInEnum { .. } => Some((v.location, "default")),
                _ => None,
            })
            .collect();

        assert_eq!(
            located,
            [
                (format!("{server}/variables/a~1b~0c"), "empty"),
                (format!("{server}/variables/d"), "default"),
            ],
            "{server}"
        );
    }
}

/// `/users` with `GET listUsers`, whose `200` response carries `link` as
/// `next`.
fn document_linking(link: crate::model::link::Link) -> Document {
    let operation = Operation::new("listUsers")
        .with_responses(Responses::new().with(200, Response::new("ok").with_link("next", link)));
    document_with(&[(
        "/users",
        PathItem::new().with_operation(Method::Get, operation),
    )])
}

/// A link names an operation the description describes.
///
/// `references/3.1.2.md:2425,2434-2435` and `references/3.2.0.md:2503`: an
/// `operationId` "MUST be resolved within the scope of the OpenAPI
/// Description", and an `operationRef` "MUST point to an Operation Object".
/// Both are reported at the link, wherever it sits: inline under an
/// operation's response, `default` and a webhook's included, or as a
/// component.
#[test]
fn a_link_naming_no_described_operation_is_reported_at_the_link() {
    use crate::model::{link::Link, reference::RefOr};

    let unknown_id = || SpecError::UnknownLinkOperationId {
        operation_id: "missing".to_owned(),
    };
    let unresolved_ref = |operation_ref: &str| SpecError::UnresolvedLinkOperationRef {
        operation_ref: operation_ref.to_owned(),
    };
    let inline = "#/paths/~1users/get/responses/200/links/next";

    let mut component_link = document_linking(Link::to_operation("listUsers"));
    component_link.components.links.insert(
        "Next".to_owned(),
        RefOr::Item(Link::to_operation("missing")),
    );

    let mut component_response = document_linking(Link::to_operation("listUsers"));
    component_response.components.responses.insert(
        "Page".to_owned(),
        RefOr::Item(
            Response::new("ok").with_link("next", Link::to_operation_ref("#/paths/~1users")),
        ),
    );

    let mut webhook = document_linking(Link::to_operation("listUsers"));
    webhook.webhooks.insert(
        "hook".to_owned(),
        PathItem::new().with_operation(
            Method::Post,
            Operation::new("onHook").with_responses(
                Responses::new().with(
                    200,
                    Response::new("ok")
                        .with_link("next", Link::to_operation_ref("#/paths/~1users/post")),
                ),
            ),
        ),
    );

    let fallback = document_with(&[(
        "/users",
        PathItem::new().with_operation(
            Method::Get,
            Operation::new("listUsers").with_responses(Responses::new().with_default(
                Response::new("ok").with_link("next", Link::to_operation("missing")),
            )),
        ),
    )]);

    let cases = [
        (
            document_linking(Link::to_operation("missing")),
            inline,
            unknown_id(),
        ),
        (
            fallback,
            "#/paths/~1users/get/responses/default/links/next",
            unknown_id(),
        ),
        (
            document_linking(Link::to_operation_ref("#/paths/~1nope/get")),
            inline,
            unresolved_ref("#/paths/~1nope/get"),
        ),
        // A malformed escape decodes to no pointer at all.
        (
            document_linking(Link::to_operation_ref("#/paths/~1users/%zzget")),
            inline,
            unresolved_ref("#/paths/~1users/%zzget"),
        ),
        (component_link, "#/components/links/Next", unknown_id()),
        // A Path Item is not an Operation Object.
        (
            component_response,
            "#/components/responses/Page/links/next",
            unresolved_ref("#/paths/~1users"),
        ),
        (
            webhook,
            "#/webhooks/hook/post/responses/200/links/next",
            unresolved_ref("#/paths/~1users/post"),
        ),
    ];

    for (document, location, error) in cases {
        assert_eq!(
            located_errors(&document),
            vec![(location.to_owned(), error)],
            "{location}"
        );
    }
}

/// A link resolves wherever the operation is described, and is not judged
/// where the operation may lie beyond this document.
///
/// `references/3.1.2.md:185-186` resolves an `operationId` against "all
/// Operation Objects from all parsed documents", so a Path Item or Callback
/// referenced from another document leaves every identifier open; an
/// `operationRef` into another document, or through a `$ref` this walk does
/// not follow, likewise.
#[test]
#[expect(clippy::too_many_lines)]
fn a_link_to_a_described_or_unknowable_operation_is_not_reported() {
    use crate::model::{
        callback::Callback,
        link::Link,
        reference::{Ref, RefOr},
    };

    let operation = |id: &str| Operation::new(id).with_responses(ok_responses());
    let mut listing = operation("listUsers");
    listing.callbacks.insert(
        "onEvent".to_owned(),
        RefOr::Item(Callback::new().with(
            "{$request.body#/url}",
            PathItem::new().with_operation(Method::Post, operation("notify")),
        )),
    );
    listing.callbacks.insert(
        "relay".to_owned(),
        RefOr::Ref(Ref::new("#/components/callbacks/Relay")),
    );

    let mut document = document_with(&[
        (
            "/users",
            PathItem::new().with_operation(Method::Get, listing),
        ),
        (
            "/users/{id}",
            PathItem::new().with_operation(
                Method::Get,
                operation("getUser")
                    .with_parameter(Parameter::path("id", Schema::of_type(SchemaType::String))),
            ),
        ),
    ]);
    document.webhooks.insert(
        "hook".to_owned(),
        PathItem::new().with_operation(Method::Post, operation("onHook")),
    );
    document.components.path_items.insert(
        "Shared".to_owned(),
        PathItem::new().with_operation(Method::Get, operation("shared")),
    );
    let mut proxy = PathItem::new();
    proxy.reference = Some("#/components/pathItems/Shared".to_owned());
    let template = PathTemplate::parse("/proxy").expect("valid template");
    document.paths.insert(&template, proxy);
    // A component callback declares `relayNotify` only there, and its second
    // Path Item is given by reference.
    let mut relay = Callback::new().with(
        "{$request.body#/url}",
        PathItem::new().with_operation(Method::Post, operation("relayNotify")),
    );
    relay.items.insert(
        "{$request.query.cb}".to_owned(),
        RefOr::Ref(Ref::new("#/components/pathItems/Shared")),
    );
    document
        .components
        .callbacks
        .insert("Relay".to_owned(), RefOr::Item(relay));

    let links = [
        Link::to_operation("listUsers"),
        Link::to_operation("notify"),
        Link::to_operation("onHook"),
        Link::to_operation("shared"),
        Link::to_operation_ref("#/paths/~1users/get"),
        Link::to_operation_ref("#/paths/~1users~1%7Bid%7D/get"),
        Link::to_operation_ref(
            "#/paths/~1users/get/callbacks/onEvent/%7B$request.body%23~1url%7D/post",
        ),
        Link::to_operation_ref("#/webhooks/hook/post"),
        Link::to_operation_ref("#/components/pathItems/Shared/get"),
        Link::to_operation_ref("#/paths/~1proxy/get"),
        Link::to_operation_ref("#/paths/~1users/get/callbacks/relay/x/post"),
        Link::to_operation("relayNotify"),
        Link::to_operation_ref("#/components/callbacks/Relay/%7B$request.body%23~1url%7D/post"),
        Link::to_operation_ref("#/components/callbacks/Relay/%7B$request.query.cb%7D/get"),
        Link::to_operation_ref("https://example.com/openapi.json#/paths/~1nope/get"),
        Link::to_operation_ref("other.json#/paths/~1nope/get"),
    ];
    for (index, link) in links.into_iter().enumerate() {
        document
            .components
            .links
            .insert(format!("link{index}"), RefOr::Item(link));
    }
    assert_eq!(located_errors(&document), vec![]);

    // An identifier no part of this document declares may be declared by the
    // document a Path Item refers to.
    let mut remote = PathItem::new();
    remote.reference = Some("other.json#/paths/~1remote".to_owned());
    let template = PathTemplate::parse("/remote").expect("valid template");
    let mut elsewhere = document_linking(Link::to_operation("remoteOnly"));
    elsewhere.paths.insert(&template, remote);
    assert_eq!(located_errors(&elsewhere), vec![]);

    // As may the document a Callback refers to.
    let mut relayed = operation("relayed");
    relayed.callbacks.insert(
        "remote".to_owned(),
        RefOr::Ref(Ref::new("other.json#/components/callbacks/Remote")),
    );
    let mut elsewhere = document_linking(Link::to_operation("remoteOnly"));
    elsewhere.paths.insert(
        &PathTemplate::parse("/relayed").expect("valid template"),
        PathItem::new().with_operation(Method::Get, relayed),
    );
    assert_eq!(located_errors(&elsewhere), vec![]);

    // 3.2's `additionalOperations` are described operations too.
    #[cfg(feature = "openapi32")]
    {
        let mut item = PathItem::new();
        item.additional_operations
            .insert("COPY".to_owned(), Box::new(operation("copyUsers")));
        let mut document = document_with(&[("/users", item)]);
        document.components.links.insert(
            "Copy".to_owned(),
            RefOr::Item(Link::to_operation_ref(
                "#/paths/~1users/additionalOperations/COPY",
            )),
        );
        assert_eq!(located_errors_3_2(&document), vec![]);
    }
}

#[test]
fn an_operation_must_declare_a_response() {
    let item = PathItem::new().with_operation(Method::Get, Operation::new("listUsers"));
    let found = errors(&document_with(&[("/users", item)]));
    assert!(matches!(found.as_slice(), [SpecError::NoResponses]));
}

/// An extension is not a response code.
///
/// `references/3.2.0.md:2109`: "The Responses Object MUST contain at least one
/// response code". `Responses::is_empty` deliberately counts extensions, and
/// argues for it: a Responses Object holding only `x-` fields is skipped whole
/// on serialization, so ignoring them there would drop it from a round trip.
/// That is right for the *predicate* and wrong for this *rule*, which reused
/// it — so `{"x-poll-interval": 30}` as a whole Responses Object satisfied a
/// requirement it plainly does not meet.
#[test]
fn extensions_alone_do_not_satisfy_the_response_requirement() {
    let mut responses = Responses::new();
    responses.extensions.insert("x-poll-interval", 30);

    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("listUsers").with_responses(responses),
    );

    let found = errors(&document_with(&[("/users", item)]));
    assert!(
        found
            .iter()
            .any(|error| matches!(error, SpecError::NoResponses)),
        "got {found:?}"
    );
}

/// 3.1 requires a response description; 3.2 does not.
///
/// `references/3.1.2.md:2010` marks `description` **REQUIRED** and
/// `references/3.2.0.md:2161` does not. The requirement used to live in the
/// type, as `description: String`, which enforced it on 3.2 as well and made a
/// legal 3.2 document unparseable. It lives here now, where it is checked
/// against the version the document claims rather than against every version at
/// once.
#[cfg(feature = "openapi32")]
#[test]
fn a_response_without_a_description_is_a_three_one_violation() {
    let summarised = Response {
        summary: Some("The order".to_owned()),
        ..Response::default()
    };
    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("getOrder").with_responses(Responses::new().with(200, summarised)),
    );
    let document = document_with(&[("/orders", item)]);

    assert!(
        errors(&document)
            .iter()
            .any(|error| matches!(error, SpecError::MissingResponseDescription)),
        "3.1 requires one"
    );

    let under_three_two: Vec<SpecError> = Validator::new(SpecVersion::V3_2)
        .validate(&document)
        .into_iter()
        .filter(|v| v.severity == super::Severity::Error)
        .map(|v| v.error)
        .collect();
    assert!(
        !under_three_two
            .iter()
            .any(|error| matches!(error, SpecError::MissingResponseDescription)),
        "3.2 does not"
    );
}

#[test]
fn security_requirements_must_name_a_declared_scheme() {
    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("listUsers")
            .with_responses(ok_responses())
            .with_security(SecurityRequirement::scheme("Bearer")),
    );

    let found = errors(&document_with(&[("/users", item)]));
    assert!(matches!(
        found.as_slice(),
        [SpecError::UnknownSecurityScheme { name }] if name == "Bearer"
    ));
}

/// 3.2 lets a requirement name a scheme by URI; 3.1 does not.
///
/// `references/3.2.0.md:4685`: the name "MUST **either** correspond to a
/// security scheme declared in the Security Schemes … **or be the URI of a
/// Security Scheme Object**". `references/3.1.2.md:4129` has only the first
/// half, and the rule was written to it and never gated — so a valid 3.2
/// document naming `./foo` is reported as an error.
///
/// A bare undeclared name stays an error under both. It is a legal relative
/// URI reference in the abstract, but 3.2 says a name matching a component
/// name is a component name and that referencing by single-segment relative
/// URI is spelled `./foo` — so the bare form is the typo the rule exists to
/// catch, and reading it as a URI would make the check vacuous.
#[cfg(feature = "openapi32")]
#[test]
fn a_three_two_requirement_may_name_a_scheme_by_uri() {
    let requiring = |name: &str| {
        let item = PathItem::new().with_operation(
            Method::Get,
            Operation::new("listUsers")
                .with_responses(ok_responses())
                .with_security(SecurityRequirement::scheme(name)),
        );
        document_with(&[("/users", item)])
    };
    let unknown_schemes = |document: &Document, version: SpecVersion| {
        Validator::new(version)
            .validate(document)
            .into_iter()
            .filter(|v| matches!(v.error, SpecError::UnknownSecurityScheme { .. }))
            .count()
    };

    for name in ["./foo", "https://example.com/schemes#/Bearer"] {
        let document = requiring(name);
        assert_eq!(
            unknown_schemes(&document, SpecVersion::V3_2),
            0,
            "3.2 permits `{name}`"
        );
        assert_eq!(
            unknown_schemes(&document, SpecVersion::V3_1),
            1,
            "3.1 does not permit `{name}`"
        );
    }

    let bare = requiring("Bearer");
    assert_eq!(
        unknown_schemes(&bare, SpecVersion::V3_2),
        1,
        "a bare undeclared name is a typo under either version"
    );
}

#[test]
fn a_declared_scheme_satisfies_the_requirement() {
    let mut document = document_with(&[(
        "/users",
        PathItem::new().with_operation(
            Method::Get,
            Operation::new("listUsers")
                .with_responses(ok_responses())
                .with_security(SecurityRequirement::scheme("Bearer")),
        ),
    )]);
    document.components.security_schemes.insert(
        "Bearer".to_owned(),
        crate::RefOr::Item(crate::SecurityScheme::bearer(None)),
    );

    assert!(errors(&document).is_empty());
}

/// The root `security` list is held to the same rule as an operation's:
/// `references/3.1.2.md:4129` says each name "MUST correspond to a security
/// scheme which is declared in the Security Schemes", wherever the
/// requirement appears.
#[test]
fn a_document_requirement_must_name_a_declared_scheme() {
    let document = document_with(&[])
        .with_security(SecurityRequirement::scheme("Bearer"))
        .with_security(SecurityRequirement::scheme("Typo"));
    let mut declared = document.clone();
    declared.components.security_schemes.insert(
        "Bearer".to_owned(),
        crate::RefOr::Item(crate::SecurityScheme::bearer(None)),
    );

    let reported: Vec<(String, SpecError)> = violations(&declared)
        .into_iter()
        .map(|v| (v.location, v.error))
        .collect();
    assert!(
        matches!(
            reported.as_slice(),
            [(location, SpecError::UnknownSecurityScheme { name })]
                if location == "#/security/1" && name == "Typo"
        ),
        "only the undeclared name is reported, at its own index: {reported:?}"
    );

    assert_eq!(
        violations(&document)
            .iter()
            .filter(|v| matches!(v.error, SpecError::UnknownSecurityScheme { .. }))
            .count(),
        2,
        "with nothing declared, both names are unknown"
    );
}

/// The 3.2 allowance for naming a scheme by URI holds at the root as well.
#[cfg(feature = "openapi32")]
#[test]
fn a_three_two_document_requirement_may_name_a_scheme_by_uri() {
    let unknown_schemes = |name: &str, version: SpecVersion| {
        Validator::new(version)
            .validate(&document_with(&[]).with_security(SecurityRequirement::scheme(name)))
            .into_iter()
            .filter(|v| matches!(v.error, SpecError::UnknownSecurityScheme { .. }))
            .count()
    };

    assert_eq!(unknown_schemes("./foo", SpecVersion::V3_2), 0);
    assert_eq!(unknown_schemes("./foo", SpecVersion::V3_1), 1);
    assert_eq!(unknown_schemes("Bearer", SpecVersion::V3_2), 1);
}

/// A same-document pointer into the Security Schemes is a URI this document
/// alone resolves, so under 3.2 it must land on a declared scheme:
/// `references/3.2.0.md:4703` says "Each name or URI MUST correspond to a
/// security scheme". Both the root and an operation's requirements are held
/// to it; a URI into another document still passes, since nothing here can
/// follow it.
#[cfg(feature = "openapi32")]
#[test]
fn a_three_two_requirement_pointer_must_resolve_to_a_declared_scheme() {
    let reported = |name: &str| {
        let item = PathItem::new().with_operation(
            Method::Get,
            Operation::new("listUsers")
                .with_responses(ok_responses())
                .with_security(SecurityRequirement::scheme(name)),
        );
        let mut document =
            document_with(&[("/users", item)]).with_security(SecurityRequirement::scheme(name));
        document.components.security_schemes.insert(
            "Bearer".to_owned(),
            crate::RefOr::Item(crate::SecurityScheme::bearer(None)),
        );
        Validator::new(SpecVersion::V3_2)
            .validate(&document)
            .into_iter()
            .filter_map(|v| match v.error {
                SpecError::UnknownSecurityScheme { name } => Some((v.location, name)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };

    let dangling = "#/components/securitySchemes/Typo";
    assert_eq!(
        reported(dangling),
        [
            ("#/security/0".to_owned(), dangling.to_owned()),
            ("#/paths/~1users/get".to_owned(), dangling.to_owned()),
        ],
        "a pointer to an undeclared scheme is reported at the root and on the operation"
    );

    for name in [
        "#/components/securitySchemes/Bearer",
        "https://example.com/schemes#/components/securitySchemes/Typo",
        "./other.yaml#/components/securitySchemes/Typo",
    ] {
        assert_eq!(reported(name), [], "`{name}` passes");
    }
}

/// Every [`SpecError::MissingOAuthFlowUrl`] `version` reports for `flows`,
/// declared as scheme `OAuth`, as `(location, flow, field)`, sorted.
fn missing_flow_urls(
    flows: crate::OAuthFlows,
    version: SpecVersion,
) -> Vec<(String, &'static str, &'static str)> {
    let mut document = document_with(&[]);
    document.components.security_schemes.insert(
        "OAuth".to_owned(),
        crate::RefOr::Item(crate::SecurityScheme::oauth2(flows)),
    );
    let mut found: Vec<_> = Validator::new(version)
        .validate(&document)
        .into_iter()
        .filter_map(|v| match v.error {
            SpecError::MissingOAuthFlowUrl { flow, field } => Some((v.location, flow, field)),
            _ => None,
        })
        .collect();
    found.sort_unstable();
    found
}

/// `references/3.1.2.md:4078-4079` makes `authorizationUrl` REQUIRED for
/// `implicit` and `authorizationCode`, and `tokenUrl` for `password`,
/// `clientCredentials` and `authorizationCode`. Each missing URL is its own
/// violation, at the flow that omits it.
#[test]
fn each_oauth_flow_must_declare_its_required_urls() {
    use crate::{OAuthFlow, OAuthFlows};

    let bare = || OAuthFlow::new([]);
    let at = |flow: &str| format!("#/components/securitySchemes/OAuth/flows/{flow}");
    let mut expected = vec![
        (at("implicit"), "implicit", "authorizationUrl"),
        (at("password"), "password", "tokenUrl"),
        (at("clientCredentials"), "clientCredentials", "tokenUrl"),
        (
            at("authorizationCode"),
            "authorizationCode",
            "authorizationUrl",
        ),
        (at("authorizationCode"), "authorizationCode", "tokenUrl"),
    ];
    expected.sort_unstable();
    let flows = OAuthFlows::default()
        .with_implicit(bare())
        .with_password(bare())
        .with_client_credentials(bare())
        .with_authorization_code(bare());
    assert_eq!(
        missing_flow_urls(flows, SpecVersion::V3_1),
        expected,
        "every missing URL is reported"
    );

    let complete = OAuthFlows::default()
        .with_implicit(bare().with_authorization_url("https://example.com/authorize"))
        .with_password(bare().with_token_url("https://example.com/token"))
        .with_client_credentials(bare().with_token_url("https://example.com/token"))
        .with_authorization_code(
            bare()
                .with_authorization_url("https://example.com/authorize")
                .with_token_url("https://example.com/token"),
        );
    assert_eq!(
        missing_flow_urls(complete, SpecVersion::V3_1),
        [],
        "a flow declaring its URLs passes"
    );
}

/// `references/3.2.0.md:4655-4657` makes `deviceAuthorizationUrl` and
/// `tokenUrl` REQUIRED for `deviceAuthorization`.
#[cfg(feature = "openapi32")]
#[test]
fn the_device_authorization_flow_must_declare_its_required_urls() {
    use crate::{OAuthFlow, OAuthFlows};

    let at = "#/components/securitySchemes/OAuth/flows/deviceAuthorization";
    let flows = OAuthFlows::default().with_device_authorization(OAuthFlow::new([]));
    assert_eq!(
        missing_flow_urls(flows, SpecVersion::V3_2),
        [
            (
                at.to_owned(),
                "deviceAuthorization",
                "deviceAuthorizationUrl"
            ),
            (at.to_owned(), "deviceAuthorization", "tokenUrl"),
        ],
    );

    let complete = OAuthFlows::default().with_device_authorization(
        OAuthFlow::new([])
            .with_device_authorization_url("https://example.com/device")
            .with_token_url("https://example.com/token"),
    );
    assert_eq!(missing_flow_urls(complete, SpecVersion::V3_2), []);
}

#[test]
fn an_unconstrained_schema_is_a_warning_not_an_error() {
    let mut operation = Operation::new("ingest").with_responses(ok_responses());
    operation.request_body = Some(crate::RefOr::Item(crate::RequestBody::json(Schema::any())));

    let document = document_with(&[(
        "/ingest",
        PathItem::new().with_operation(Method::Post, operation),
    )]);

    assert!(errors(&document).is_empty());

    let reported = Validator::new(SpecVersion::V3_1).validate(&document);
    assert!(
        reported
            .iter()
            .any(|v: &Violation| v.severity == super::Severity::Warning
                && matches!(v.error, SpecError::UncheckedSchema)),
        "expected an unchecked-schema warning, got {reported:?}"
    );
}

#[test]
fn using_an_undeclared_tag_is_a_warning() {
    let document = document_with(&[(
        "/users",
        PathItem::new().with_operation(
            Method::Get,
            Operation::new("listUsers")
                .with_tag("users")
                .with_responses(ok_responses()),
        ),
    )]);

    assert!(errors(&document).is_empty());
    assert!(
        Validator::new(SpecVersion::V3_1)
            .validate(&document)
            .iter()
            .any(|v| matches!(v.error, SpecError::UndocumentedTag { .. }))
    );
}

#[test]
fn duplicate_tag_names_are_rejected() {
    let mut document = document_with(&[]);
    document.tags.push(crate::Tag::new("users"));
    document.tags.push(crate::Tag::new("users"));

    assert!(
        errors(&document)
            .iter()
            .any(|e| matches!(e, SpecError::DuplicateTag { .. }))
    );
}

#[test]
fn validate_reports_errors_and_hides_warnings() {
    let item = PathItem::new().with_operation(Method::Get, Operation::new("listUsers"));
    let document = document_with(&[("/users", item)]);
    let result = document.validate(SpecVersion::V3_1);
    let reported = result.expect_err("has an error");
    assert!(
        reported
            .iter()
            .all(|v| v.severity == super::Severity::Error)
    );
}

fn violations(document: &Document) -> Vec<Violation> {
    Validator::new(SpecVersion::V3_1).validate(document)
}

fn opaque_document() -> Document {
    let mut operation = Operation::new("listUsers").with_responses(ok_responses());
    crate::annotation::Opaque::new(crate::annotation::OpaqueReason::UntypedLayer)
        .apply_to(&mut operation)
        .expect("nothing to conflict with");
    let item = PathItem::new().with_operation(Method::Get, operation);
    let mut document = document_with(&[("/users", item)]);
    document.restamp_authority();
    document
}

#[test]
fn an_opaque_operation_is_a_warning_not_an_error() {
    let document = opaque_document();

    assert!(errors(&document).is_empty());
    assert!(violations(&document).iter().any(|v| {
        v.severity == super::Severity::Warning
            && v.location == "#/paths/~1users/get"
            && matches!(v.error, SpecError::OpaqueOperation { .. })
    }));
}

#[test]
fn an_opaque_document_that_omits_the_stamp_is_an_error() {
    let mut document = opaque_document();
    document
        .extensions
        .0
        .shift_remove(crate::annotation::NOT_AUTHORITATIVE_ANNOTATION);

    assert!(
        errors(&document)
            .iter()
            .any(|e| matches!(e, SpecError::AuthorityNotStamped))
    );
}

#[test]
fn an_opaque_route_is_reported_without_inventing_a_paths_entry() {
    let mut document = document_with(&[(
        "/users",
        PathItem::new().with_operation(
            Method::Get,
            Operation::new("listUsers").with_responses(ok_responses()),
        ),
    )]);
    crate::annotation::OpaqueRoute::new(
        "/assets/{*path}",
        crate::annotation::OpaqueReason::UntypedRoute,
    )
    .append_to(&mut document)
    .expect("nothing to conflict with");
    document.restamp_authority();

    assert!(errors(&document).is_empty());
    assert!(violations(&document).iter().any(|v| {
        matches!(&v.error, SpecError::OpaqueRoute { pattern } if pattern == "/assets/{*path}")
    }));
    // The route is recorded, and `paths` is untouched by it.
    assert_eq!(document.paths.items.len(), 1);
}

#[test]
fn an_unreadable_annotation_is_an_error() {
    let mut document = document_with(&[(
        "/users",
        PathItem::new().with_operation(
            Method::Get,
            Operation::new("listUsers").with_responses(ok_responses()),
        ),
    )]);
    document.extensions.insert(
        crate::annotation::OPAQUE_ROUTES_ANNOTATION,
        serde_json::json!("nonsense"),
    );

    let reported = errors(&document);
    assert!(reported.iter().any(
        |e| matches!(e, SpecError::MalformedAnnotation { name, .. } if name == "x-kynos-opaque-routes")
    ));
    // Unreadable is not clean, so the authority stamp is demanded too.
    assert!(
        reported
            .iter()
            .any(|e| matches!(e, SpecError::AuthorityNotStamped))
    );
}

#[test]
fn a_clean_document_makes_no_opacity_noise() {
    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("health").with_responses(ok_responses()),
    );
    let document = document_with(&[("/health", item)]);

    assert!(!violations(&document).iter().any(|v| matches!(
        v.error,
        SpecError::NotAuthoritative
            | SpecError::AuthorityNotStamped
            | SpecError::StaleAuthorityStamp
            | SpecError::OpaqueOperation { .. }
            | SpecError::OpaqueRoute { .. }
    )));
}

/// The stamp is true exactly when something is opaque, so one left on a
/// document with nothing opaque is stale. It understates the description,
/// which is worth saying but not worth failing over.
#[test]
fn a_stale_authority_stamp_on_an_authoritative_document_is_a_warning() {
    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("health").with_responses(ok_responses()),
    );
    let mut document = document_with(&[("/health", item)]);
    document
        .extensions
        .insert(crate::annotation::NOT_AUTHORITATIVE_ANNOTATION, true);

    assert!(errors(&document).is_empty());
    let opacity: Vec<_> = violations(&document)
        .into_iter()
        .filter(|v| {
            matches!(
                v.error,
                SpecError::NotAuthoritative | SpecError::StaleAuthorityStamp
            )
        })
        .map(|v| (v.severity, v.location, v.error))
        .collect();
    assert_eq!(
        opacity,
        vec![(
            super::Severity::Warning,
            "#".to_owned(),
            SpecError::StaleAuthorityStamp
        )]
    );

    document.restamp_authority();
    assert!(
        !violations(&document)
            .iter()
            .any(|v| matches!(v.error, SpecError::StaleAuthorityStamp))
    );
}

/// Only a stamp claiming non-authority is stale; `false` claims nothing false.
#[test]
fn a_false_authority_stamp_on_an_authoritative_document_is_not_reported() {
    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("health").with_responses(ok_responses()),
    );
    let mut document = document_with(&[("/health", item)]);
    document
        .extensions
        .insert(crate::annotation::NOT_AUTHORITATIVE_ANNOTATION, false);

    assert!(
        !violations(&document)
            .iter()
            .any(|v| matches!(v.error, SpecError::StaleAuthorityStamp))
    );
}

/// A description read from somewhere else never passes through
/// `PathTemplate::parse`, so validation is the only place its keys are checked.
#[test]
fn a_paths_key_that_is_not_a_template_is_reported() {
    let document: Document = serde_json::from_value(serde_json::json!({
        "openapi": "3.1.1",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/a b|c": { "get": { "operationId": "odd", "responses": { "200": { "description": "ok" } } } }
        }
    }))
    .expect("a `paths` key is a plain string, so this parses");

    // The reason is the parse failure itself, not its text, so this can name
    // which rule the key broke rather than matching on a formatted string.
    assert!(errors(&document).iter().any(|e| matches!(
        e,
        SpecError::InvalidPathTemplate {
            template,
            reason: crate::model::paths::template::InvalidPathTemplate::IllegalLiteralCharacter { .. },
        } if template == "/a b|c"
    )));
}

/// The location must be a resolvable JSON Pointer, which means the `/` inside a
/// path key is escaped rather than left to read as a token separator.
#[test]
fn violation_locations_escape_their_path_keys() {
    let item = PathItem::new().with_operation(Method::Get, Operation::new("listUsers"));
    let document = document_with(&[("/users/{id}", item)]);

    assert!(
        violations(&document)
            .iter()
            .any(|v| v.location == "#/paths/~1users~1{id}/get")
    );
}

/// `Operation.responses` is skipped when it is empty, so a `Responses` that
/// carries only extensions must not read as empty — or a round trip drops it.
#[test]
fn a_responses_holding_only_extensions_is_not_empty() {
    let mut responses = Responses::new();
    assert!(responses.is_empty());

    responses
        .extensions
        .insert("x-poll-interval", serde_json::json!(30));
    assert!(!responses.is_empty());

    let operation = Operation::new("listUsers").with_responses(responses);
    let json = serde_json::to_string(&operation).expect("serializable");
    assert!(
        json.contains("x-poll-interval"),
        "the extension was dropped: {json}"
    );
}

/// `validate` promises violations "most structural first", so a caller
/// diffing two runs of one document must not see them shuffle.
#[test]
fn violation_order_does_not_depend_on_hashing() {
    let mut operation = Operation::new("listUsers").with_responses(ok_responses());
    for name in ["alpha", "beta", "gamma", "delta", "epsilon"] {
        operation =
            operation.with_parameter(Parameter::path(name, Schema::of_type(SchemaType::String)));
    }
    let document = document_with(&[(
        "/things",
        PathItem::new().with_operation(Method::Get, operation),
    )]);

    let first = errors(&document);
    for _ in 0..16 {
        assert_eq!(errors(&document), first);
    }
}

/// A violation is printed, not walked: `Router::validate` hands back a list and
/// `Error::Invalid` renders one, so `Display` has to carry the whole thing.
/// Offering the `SpecError` it already names as a cause as well would make every
/// reporter print that sentence twice.
#[test]
fn a_violation_says_everything_in_one_line() {
    let violation = Violation::error("/paths/~1users/get", SpecError::NoResponses);

    let rendered = violation.to_string();

    assert!(rendered.contains("/paths/~1users/get"), "{rendered}");
    assert!(
        rendered.contains(&SpecError::NoResponses.to_string()),
        "{rendered}"
    );
    assert!(std::error::Error::source(&violation).is_none());
}

// --- `in: querystring` ----------------------------------------------------

/// A querystring parameter described the way 3.2 requires: by `content`.
#[cfg(feature = "openapi32")]
fn querystring(name: &str) -> Parameter {
    use crate::model::{body::media_type::MediaType, parameter::ParameterIn};

    Parameter::with_content(
        name,
        ParameterIn::Querystring,
        "application/x-www-form-urlencoded",
        MediaType::new(Schema::of_type(SchemaType::Object)),
    )
}

/// The querystring violations a document raises, with where each was raised.
#[cfg(feature = "openapi32")]
fn querystring_violations(document: &Document) -> Vec<(String, SpecError)> {
    Validator::new(SpecVersion::V3_2)
        .validate(document)
        .into_iter()
        .filter(|v| {
            matches!(
                v.error,
                SpecError::QuerystringWithoutContent { .. }
                    | SpecError::DuplicateQuerystring { .. }
                    | SpecError::QueryBesideQuerystring { .. }
            )
        })
        .map(|v| (v.location, v.error))
        .collect()
}

#[cfg(feature = "openapi32")]
#[test]
fn a_lone_querystring_parameter_described_by_content_is_valid() {
    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("listUsers")
            .with_parameter(querystring("filter"))
            .with_parameter(Parameter::header(
                "X-Trace",
                Schema::of_type(SchemaType::String),
            ))
            .with_responses(ok_responses()),
    );

    let violations =
        Validator::new(SpecVersion::V3_2).validate(&document_with(&[("/users", item)]));
    assert!(violations.is_empty(), "{violations:?}");
}

#[cfg(feature = "openapi32")]
#[test]
fn each_querystring_rule_names_the_parameters_it_is_about() {
    use crate::model::parameter::ParameterIn;

    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("listUsers")
            .with_parameter(Parameter::query("q", Schema::of_type(SchemaType::String)))
            .with_parameter(querystring("filter"))
            .with_parameter(Parameter::new(
                "sort",
                ParameterIn::Querystring,
                Schema::of_type(SchemaType::String),
            ))
            .with_responses(ok_responses()),
    );

    let at = "#/paths/~1users/get".to_owned();
    assert_eq!(
        querystring_violations(&document_with(&[("/users", item)])),
        vec![
            (
                at.clone(),
                SpecError::QuerystringWithoutContent {
                    name: "sort".to_owned()
                }
            ),
            (
                at.clone(),
                SpecError::DuplicateQuerystring {
                    first: "filter".to_owned(),
                    second: "sort".to_owned(),
                }
            ),
            (
                at,
                SpecError::QueryBesideQuerystring {
                    query: "q".to_owned(),
                    querystring: "filter".to_owned(),
                }
            ),
        ]
    );
}

/// 3.2 forbids the pair "in the same operation (or in the operation's
/// path-item)", so a conflict split across the two is still one.
#[cfg(feature = "openapi32")]
#[test]
fn querystring_rules_count_the_path_items_parameters() {
    use crate::model::reference::RefOr;

    let mut item = PathItem::new()
        .with_operation(
            Method::Get,
            Operation::new("listUsers")
                .with_parameter(Parameter::query("q", Schema::of_type(SchemaType::String)))
                .with_responses(ok_responses()),
        )
        .with_operation(
            Method::Post,
            Operation::new("searchUsers")
                .with_parameter(querystring("sort"))
                .with_responses(ok_responses()),
        );
    item.parameters.push(RefOr::Item(querystring("filter")));

    assert_eq!(
        querystring_violations(&document_with(&[("/users", item)])),
        vec![
            (
                "#/paths/~1users/get".to_owned(),
                SpecError::QueryBesideQuerystring {
                    query: "q".to_owned(),
                    querystring: "filter".to_owned(),
                }
            ),
            (
                "#/paths/~1users/post".to_owned(),
                SpecError::DuplicateQuerystring {
                    first: "filter".to_owned(),
                    second: "sort".to_owned(),
                }
            ),
        ]
    );
}

/// An operation's parameter replaces its path item's of the same name and
/// location, so redeclaring the querystring parameter leaves one, not two.
#[cfg(feature = "openapi32")]
#[test]
fn an_operation_overriding_its_path_items_querystring_parameter_declares_one() {
    use crate::model::reference::RefOr;

    let mut item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("listUsers")
            .with_parameter(querystring("filter"))
            .with_responses(ok_responses()),
    );
    item.parameters.push(RefOr::Item(querystring("filter")));

    assert_eq!(
        querystring_violations(&document_with(&[("/users", item)])),
        vec![]
    );
}

/// A conflict wholly inside the path item's own list is the path item's, and
/// is not repeated once per operation that inherits it.
#[cfg(feature = "openapi32")]
#[test]
fn a_conflict_within_the_path_item_is_reported_once_where_it_is() {
    use crate::model::reference::RefOr;

    let mut item = PathItem::new()
        .with_operation(
            Method::Get,
            Operation::new("listUsers").with_responses(ok_responses()),
        )
        .with_operation(
            Method::Post,
            Operation::new("searchUsers").with_responses(ok_responses()),
        );
    item.parameters.push(RefOr::Item(Parameter::query(
        "q",
        Schema::of_type(SchemaType::String),
    )));
    item.parameters.push(RefOr::Item(querystring("filter")));

    assert_eq!(
        querystring_violations(&document_with(&[("/users", item)])),
        vec![(
            "#/paths/~1users".to_owned(),
            SpecError::QueryBesideQuerystring {
                query: "q".to_owned(),
                querystring: "filter".to_owned(),
            }
        )]
    );
}

/// An operation adding a querystring parameter to a path item that already
/// conflicts is reported for what it adds: beside each inherited parameter,
/// against the querystring parameter it declares.
#[cfg(feature = "openapi32")]
#[test]
fn an_operation_adding_to_a_conflicting_path_item_is_reported_for_its_own_part() {
    use crate::model::reference::RefOr;

    let mut item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("listUsers")
            .with_parameter(querystring("sort"))
            .with_responses(ok_responses()),
    );
    item.parameters.push(RefOr::Item(Parameter::query(
        "q",
        Schema::of_type(SchemaType::String),
    )));
    item.parameters.push(RefOr::Item(querystring("filter")));

    let at = "#/paths/~1users/get".to_owned();
    assert_eq!(
        querystring_violations(&document_with(&[("/users", item)])),
        vec![
            (
                "#/paths/~1users".to_owned(),
                SpecError::QueryBesideQuerystring {
                    query: "q".to_owned(),
                    querystring: "filter".to_owned(),
                }
            ),
            (
                at.clone(),
                SpecError::DuplicateQuerystring {
                    first: "filter".to_owned(),
                    second: "sort".to_owned(),
                }
            ),
            (
                at,
                SpecError::QueryBesideQuerystring {
                    query: "q".to_owned(),
                    querystring: "sort".to_owned(),
                }
            ),
        ]
    );
}

/// A querystring parameter that arrives by `$ref` is one, for both rules that
/// compare entries: beside another querystring and beside a query parameter.
#[cfg(feature = "openapi32")]
#[test]
fn querystring_rules_resolve_a_ref_d_parameter() {
    use crate::model::reference::{Ref, RefOr};

    let mut item = PathItem::new()
        .with_operation(
            Method::Get,
            Operation::new("listUsers")
                .with_parameter(Parameter::query("q", Schema::of_type(SchemaType::String)))
                .with_responses(ok_responses()),
        )
        .with_operation(
            Method::Post,
            Operation::new("searchUsers")
                .with_parameter(querystring("sort"))
                .with_responses(ok_responses()),
        );
    item.parameters.push(RefOr::Ref(Ref::parameter("Filter")));
    let document = document_with_component_parameter(
        &[("/users", item)],
        "Filter",
        RefOr::Item(querystring("filter")),
    );

    assert_eq!(
        querystring_violations(&document),
        vec![
            (
                "#/paths/~1users/get".to_owned(),
                SpecError::QueryBesideQuerystring {
                    query: "q".to_owned(),
                    querystring: "filter".to_owned(),
                }
            ),
            (
                "#/paths/~1users/post".to_owned(),
                SpecError::DuplicateQuerystring {
                    first: "filter".to_owned(),
                    second: "sort".to_owned(),
                }
            ),
        ]
    );
}

/// An operation's `$ref`'d parameter overrides its path item's of the same
/// name and location, as an inline one does.
#[cfg(feature = "openapi32")]
#[test]
fn a_ref_d_operation_parameter_overrides_the_path_items_querystring() {
    use crate::model::reference::{Ref, RefOr};

    let mut operation = Operation::new("listUsers").with_responses(ok_responses());
    operation
        .parameters
        .push(RefOr::Ref(Ref::parameter("Filter")));
    let mut item = PathItem::new().with_operation(Method::Get, operation);
    item.parameters.push(RefOr::Item(querystring("filter")));
    let document = document_with_component_parameter(
        &[("/users", item)],
        "Filter",
        RefOr::Item(querystring("filter")),
    );

    assert_eq!(querystring_violations(&document), vec![]);
}

/// A reusable querystring parameter must be described by `content` too, and is
/// reported where it is defined rather than once per reference.
#[cfg(feature = "openapi32")]
#[test]
fn a_reusable_querystring_parameter_without_content_is_reported_at_its_definition() {
    use crate::model::{
        parameter::ParameterIn,
        reference::{Ref, RefOr},
    };

    let mut operation = Operation::new("listUsers").with_responses(ok_responses());
    operation
        .parameters
        .push(RefOr::Ref(Ref::parameter("Filter")));
    let document = document_with_component_parameter(
        &[(
            "/users",
            PathItem::new().with_operation(Method::Get, operation),
        )],
        "Filter",
        RefOr::Item(Parameter::new(
            "filter",
            ParameterIn::Querystring,
            Schema::of_type(SchemaType::String),
        )),
    );

    assert_eq!(
        querystring_violations(&document),
        vec![(
            "#/components/parameters/Filter".to_owned(),
            SpecError::QuerystringWithoutContent {
                name: "filter".to_owned()
            },
        )]
    );
}

// --- The variant ledger ---------------------------------------------------

/// A variant's name, as an exhaustive match.
///
/// This is the first of the two guards: adding a variant to [`SpecError`] stops
/// this file compiling until the variant is named here. The second guard,
/// [`every_variant_has_a_case`], is what then forces it to be *raised*.
fn variant_name(error: &SpecError) -> &'static str {
    match error {
        SpecError::DuplicateOperationId { .. } => "DuplicateOperationId",
        SpecError::DuplicatePathTemplate { .. } => "DuplicatePathTemplate",
        SpecError::DuplicateOperation { .. } => "DuplicateOperation",
        #[cfg(feature = "openapi32")]
        SpecError::FixedMethodInAdditionalOperations { .. } => "FixedMethodInAdditionalOperations",
        SpecError::InvalidPathTemplate { .. } => "InvalidPathTemplate",
        SpecError::UndeclaredPathVariable { .. } => "UndeclaredPathVariable",
        SpecError::UnusedPathParameter { .. } => "UnusedPathParameter",
        SpecError::PathParameterNotRequired { .. } => "PathParameterNotRequired",
        SpecError::DuplicateParameter { .. } => "DuplicateParameter",
        #[cfg(feature = "openapi32")]
        SpecError::QuerystringWithoutContent { .. } => "QuerystringWithoutContent",
        #[cfg(feature = "openapi32")]
        SpecError::DuplicateQuerystring { .. } => "DuplicateQuerystring",
        #[cfg(feature = "openapi32")]
        SpecError::QueryBesideQuerystring { .. } => "QueryBesideQuerystring",
        SpecError::ShortCircuitMismatch { .. } => "ShortCircuitMismatch",
        SpecError::IllegalStyle { .. } => "IllegalStyle",
        SpecError::IgnoredHeaderParameter { .. } => "IgnoredHeaderParameter",
        SpecError::IgnoredHeader { .. } => "IgnoredHeader",
        SpecError::NoResponses => "NoResponses",
        SpecError::InvalidResponseKey { .. } => "InvalidResponseKey",
        SpecError::MissingResponseDescription => "MissingResponseDescription",
        #[cfg(feature = "openapi32")]
        SpecError::ConflictingEncoding => "ConflictingEncoding",
        #[cfg(feature = "openapi32")]
        SpecError::PositionalEncodingWithoutArray => "PositionalEncodingWithoutArray",
        SpecError::UnknownEncodingProperty { .. } => "UnknownEncodingProperty",
        SpecError::InvalidComponentName { .. } => "InvalidComponentName",
        SpecError::DuplicateTag { .. } => "DuplicateTag",
        SpecError::UnknownTagParent { .. } => "UnknownTagParent",
        SpecError::TagParentCycle { .. } => "TagParentCycle",
        SpecError::UnknownSecurityScheme { .. } => "UnknownSecurityScheme",
        SpecError::UnknownLinkOperationId { .. } => "UnknownLinkOperationId",
        SpecError::UnresolvedLinkOperationRef { .. } => "UnresolvedLinkOperationRef",
        SpecError::MissingOAuthFlowUrl { .. } => "MissingOAuthFlowUrl",
        SpecError::UndocumentedTag { .. } => "UndocumentedTag",
        SpecError::EmptyServerVariableEnum { .. } => "EmptyServerVariableEnum",
        SpecError::ServerVariableDefaultNotInEnum { .. } => "ServerVariableDefaultNotInEnum",
        SpecError::InvalidExtensionName { .. } => "InvalidExtensionName",
        SpecError::CallbackKeyedAsExtension { .. } => "CallbackKeyedAsExtension",
        SpecError::DiscriminatorWithoutComposition => "DiscriminatorWithoutComposition",
        SpecError::DiscriminatorPropertyNotRequired { .. } => "DiscriminatorPropertyNotRequired",
        #[cfg(feature = "openapi32")]
        SpecError::DiscriminatorWithoutDefaultMapping { .. } => {
            "DiscriminatorWithoutDefaultMapping"
        }
        SpecError::UncheckedSchema => "UncheckedSchema",
        SpecError::NotAuthoritative => "NotAuthoritative",
        SpecError::OpaqueOperation { .. } => "OpaqueOperation",
        SpecError::OpaqueRoute { .. } => "OpaqueRoute",
        SpecError::RouteConflict { .. } => "RouteConflict",
        SpecError::InvalidRoutePattern { .. } => "InvalidRoutePattern",
        SpecError::AuthorityNotStamped => "AuthorityNotStamped",
        SpecError::StaleAuthorityStamp => "StaleAuthorityStamp",
        SpecError::MalformedAnnotation { .. } => "MalformedAnnotation",
        SpecError::RequiresV3_2 { .. } => "RequiresV3_2",
    }
}

/// Variants no validation run raises, and what raises each instead.
///
/// Every one of these is reachable — none is a placeholder. Listing them is
/// what keeps [`every_variant_has_a_case`] a statement about this validator
/// rather than about the enum.
const RAISED_ELSEWHERE: &[&str] = &[
    // Prevented by the model rather than reported: a `PathItem` has one slot
    // per method, so no document holds a second operation for one. `kynos`'s
    // `Router::describe` raises it when two mounts claim the same slot, and
    // `crates/kynos/tests/routing.rs` covers it there.
    "DuplicateOperation",
    // About the router's match table, which no document has: `kynos`'s
    // `TrialTable` raises both, and `crates/kynos/tests/routing.rs` and
    // `crates/kynos/tests/unchecked.rs` cover them there.
    "RouteConflict",
    "InvalidRoutePattern",
    // `encoding` conflicts with `prefixEncoding` and `itemEncoding`,
    // neither of which a 3.1 build has.
    #[cfg(not(feature = "openapi32"))]
    "ConflictingEncoding",
    // As are the two positional encoding fields this one needs an array for.
    #[cfg(not(feature = "openapi32"))]
    "PositionalEncodingWithoutArray",
    // And `in: querystring` is a 3.2 location, so a 3.1 build has no parameter
    // to raise any of these three with.
    #[cfg(not(feature = "openapi32"))]
    "QuerystringWithoutContent",
    #[cfg(not(feature = "openapi32"))]
    "DuplicateQuerystring",
    #[cfg(not(feature = "openapi32"))]
    "QueryBesideQuerystring",
    // `additionalOperations` is a 3.2 field, absent from a 3.1 build.
    #[cfg(not(feature = "openapi32"))]
    "FixedMethodInAdditionalOperations",
    // `kynos`'s `short_circuit_mismatch` compares an interceptor's declared
    // statuses against the responses it describes. No document reaches it, and
    // `crates/kynos/src/response/mod.rs` covers it where it lives.
    "ShortCircuitMismatch",
    // Tag hierarchy arrived in 3.2, so `check_tag_hierarchy` is compiled out of
    // a baseline build along with the only code paths that raise these two.
    #[cfg(not(feature = "openapi32"))]
    "UnknownTagParent",
    #[cfg(not(feature = "openapi32"))]
    "TagParentCycle",
    // `defaultMapping` is a 3.2 field, and only validating as 3.2 lets a
    // discriminating property be optional with one.
    #[cfg(not(feature = "openapi32"))]
    "DiscriminatorWithoutDefaultMapping",
];

/// One document per variant, each named by the variant it must raise and the
/// version to validate it at.
///
/// Split by the rule module that raises each group, which is also the order
/// `Validator::validate` runs them in.
fn ledger() -> Vec<(&'static str, SpecVersion, Document)> {
    let mut cases = ledger_paths();
    cases.extend(ledger_parameters());
    #[cfg(feature = "openapi32")]
    cases.extend(ledger_querystring());
    cases.extend(ledger_document());
    cases.extend(ledger_opacity());
    cases.extend(ledger_discriminator());
    cases
}

/// Cases for the path, template, parameter and link target rules.
fn ledger_paths() -> Vec<(&'static str, SpecVersion, Document)> {
    use crate::model::schema::Schema;

    let operation = || Operation::new("listUsers").with_responses(ok_responses());
    let get = |operation: Operation| PathItem::new().with_operation(Method::Get, operation);

    let mut cases: Vec<(&'static str, SpecVersion, Document)> = Vec::new();
    let mut push =
        |name: &'static str, document: Document| cases.push((name, SpecVersion::V3_1, document));

    push(
        "DuplicateOperationId",
        document_with(&[("/a", get(operation())), ("/b", get(operation()))]),
    );
    push(
        "DuplicatePathTemplate",
        document_with(&[
            (
                "/pets/{petId}",
                get(Operation::new("a").with_responses(ok_responses())),
            ),
            (
                "/pets/{name}",
                get(Operation::new("b").with_responses(ok_responses())),
            ),
        ]),
    );
    push(
        "InvalidPathTemplate",
        serde_json::from_value(serde_json::json!({
            "openapi": "3.1.2",
            "info": { "title": "T", "version": "1" },
            "paths": { "/a b|c": { "get": { "responses": { "200": { "description": "ok" } } } } }
        }))
        .expect("a `paths` key is a plain string, so this parses"),
    );
    push(
        "UndeclaredPathVariable",
        document_with(&[("/users/{id}", get(operation()))]),
    );
    push(
        "UnusedPathParameter",
        document_with(&[(
            "/users",
            get(operation()
                .with_parameter(Parameter::path("id", Schema::of_type(SchemaType::String)))),
        )]),
    );
    push(
        "PathParameterNotRequired",
        document_with(&[(
            "/users/{id}",
            get(operation().with_parameter({
                let mut parameter = Parameter::path("id", Schema::of_type(SchemaType::String));
                parameter.required = None;
                parameter
            })),
        )]),
    );
    push(
        "InvalidResponseKey",
        document_with(&[(
            "/users",
            get(Operation::new("listUsers")
                .with_responses(Responses::new().with(600, Response::new("ok")))),
        )]),
    );
    #[cfg(feature = "openapi32")]
    push("FixedMethodInAdditionalOperations", {
        let mut item = PathItem::new();
        item.additional_operations
            .insert("POST".to_owned(), Box::new(operation()));
        document_with(&[("/users", item)])
    });
    push(
        "UnknownLinkOperationId",
        document_linking(crate::model::link::Link::to_operation("missing")),
    );
    push(
        "UnresolvedLinkOperationRef",
        document_linking(crate::model::link::Link::to_operation_ref(
            "#/paths/~1nope/get",
        )),
    );
    cases
}

/// Cases for the parameter and header rules.
fn ledger_parameters() -> Vec<(&'static str, SpecVersion, Document)> {
    use crate::model::{parameter::header::Header, schema::Schema};

    let operation = || Operation::new("listUsers").with_responses(ok_responses());
    let get = |operation: Operation| PathItem::new().with_operation(Method::Get, operation);

    let mut cases: Vec<(&'static str, SpecVersion, Document)> = Vec::new();
    let mut push =
        |name: &'static str, document: Document| cases.push((name, SpecVersion::V3_1, document));

    push(
        "DuplicateParameter",
        document_with(&[(
            "/users",
            get(operation()
                .with_parameter(Parameter::query("q", Schema::of_type(SchemaType::String)))
                .with_parameter(Parameter::query("q", Schema::of_type(SchemaType::String)))),
        )]),
    );
    push(
        "IllegalStyle",
        document_with(&[(
            "/users",
            get(operation().with_parameter(
                Parameter::query("q", Schema::of_type(SchemaType::String))
                    .with_style(Style::Simple, false),
            )),
        )]),
    );
    push(
        "IgnoredHeaderParameter",
        document_with(&[(
            "/users",
            get(operation().with_parameter(Parameter::header(
                "Accept",
                Schema::of_type(SchemaType::String),
            ))),
        )]),
    );
    push(
        "IgnoredHeader",
        document_with(&[(
            "/users",
            get(operation().with_responses(Responses::new().with(
                200,
                Response::new("ok").with_header(
                    "Content-Type",
                    Header::new(Schema::of_type(SchemaType::String)),
                ),
            ))),
        )]),
    );
    push(
        "NoResponses",
        document_with(&[("/users", get(Operation::new("listUsers")))]),
    );
    // A response with no description at all, which is the whole of what this
    // rule is about. `summary` would make it a *useful* 3.2 document, and is
    // 3.2-only — so naming it here would leave the variant with no case at
    // baseline features, where the rule fires just the same.
    push(
        "MissingResponseDescription",
        document_with(&[(
            "/users",
            get(Operation::new("listUsers")
                .with_responses(Responses::new().with(200, Response::default()))),
        )]),
    );
    // 3.2 only: `prefixEncoding` is what `encoding` conflicts with, and it does
    // not exist under 3.1, so neither does the conflict.
    #[cfg(feature = "openapi32")]
    push(
        "ConflictingEncoding",
        document_with_upload("multipart/mixed", conflicting_media_type()),
    );
    #[cfg(feature = "openapi32")]
    push("PositionalEncodingWithoutArray", {
        use crate::model::body::{encoding::Encoding, media_type::MediaType};

        let mut content = MediaType::new(Schema::of_type(SchemaType::Object));
        content.item_encoding = Some(Box::new(Encoding::new("text/plain")));
        document_with_upload("multipart/mixed", content)
    });
    push("UnknownEncodingProperty", {
        use crate::model::body::{encoding::Encoding, media_type::MediaType};

        document_with_upload(
            "multipart/form-data",
            MediaType::new(object_with(&["file"]))
                .with_encoding("fiel", Encoding::new("text/plain")),
        )
    });
    cases
}

/// Cases for the `in: querystring` rules, which are 3.2 only, like the location
/// all three are about.
#[cfg(feature = "openapi32")]
fn ledger_querystring() -> Vec<(&'static str, SpecVersion, Document)> {
    use crate::model::{parameter::ParameterIn, schema::Schema};

    let operation = || Operation::new("listUsers").with_responses(ok_responses());
    let get = |operation: Operation| PathItem::new().with_operation(Method::Get, operation);

    vec![
        (
            "QuerystringWithoutContent",
            SpecVersion::V3_2,
            document_with(&[(
                "/users",
                get(operation().with_parameter(Parameter::new(
                    "filter",
                    ParameterIn::Querystring,
                    Schema::of_type(SchemaType::Object),
                ))),
            )]),
        ),
        (
            "DuplicateQuerystring",
            SpecVersion::V3_2,
            document_with(&[(
                "/users",
                get(operation()
                    .with_parameter(querystring("filter"))
                    .with_parameter(querystring("sort"))),
            )]),
        ),
        (
            "QueryBesideQuerystring",
            SpecVersion::V3_2,
            document_with(&[(
                "/users",
                get(operation()
                    .with_parameter(Parameter::query("q", Schema::of_type(SchemaType::String)))
                    .with_parameter(querystring("filter"))),
            )]),
        ),
    ]
}

/// Cases for the whole-document rules: components, tags, servers, security.
fn ledger_document() -> Vec<(&'static str, SpecVersion, Document)> {
    use crate::model::{
        schema::Schema,
        server::{Server, ServerVariable},
        tag::Tag,
    };

    let operation = || Operation::new("listUsers").with_responses(ok_responses());
    let get = |operation: Operation| PathItem::new().with_operation(Method::Get, operation);

    let mut cases: Vec<(&'static str, SpecVersion, Document)> = Vec::new();
    let mut push =
        |name: &'static str, document: Document| cases.push((name, SpecVersion::V3_1, document));

    push("InvalidComponentName", {
        let mut document = document_with(&[("/users", get(operation()))]);
        document
            .components
            .schemas
            .insert("not a name".to_owned(), Schema::of_type(SchemaType::String));
        document
    });
    push("DuplicateTag", {
        let mut document = document_with(&[("/users", get(operation()))]);
        document.tags = vec![Tag::new("orders"), Tag::new("orders")];
        document
    });
    #[cfg(feature = "openapi32")]
    push("UnknownTagParent", {
        let mut document = document_with(&[("/users", get(operation()))]);
        document.tags = vec![Tag::new("orders").with_parent("absent")];
        document
    });
    #[cfg(feature = "openapi32")]
    push("TagParentCycle", {
        let mut document = document_with(&[("/users", get(operation()))]);
        document.tags = vec![
            Tag::new("a").with_parent("b"),
            Tag::new("b").with_parent("a"),
        ];
        document
    });
    push(
        "UnknownSecurityScheme",
        document_with(&[(
            "/users",
            get(operation().with_security(SecurityRequirement::scheme("Undeclared"))),
        )]),
    );
    push("MissingOAuthFlowUrl", {
        let mut document = document_with(&[("/users", get(operation()))]);
        document.components.security_schemes.insert(
            "OAuth".to_owned(),
            crate::RefOr::Item(crate::SecurityScheme::oauth2(
                crate::OAuthFlows::default().with_password(crate::OAuthFlow::new([])),
            )),
        );
        document
    });
    push(
        "UndocumentedTag",
        document_with(&[("/users", get(operation().with_tag("orders")))]),
    );
    push("EmptyServerVariableEnum", {
        let mut document = document_with(&[("/users", get(operation()))]);
        document.servers = vec![Server::new("https://example.com/{region}").with_variable(
            "region",
            ServerVariable {
                enumeration: Some(Vec::new()),
                default_value: "eu".to_owned(),
                description: None,
                extensions: crate::model::extensions::Extensions::new(),
            },
        )];
        document
    });
    push("ServerVariableDefaultNotInEnum", {
        let mut document = document_with(&[("/users", get(operation()))]);
        document.servers = vec![Server::new("https://example.com/{region}").with_variable(
            "region",
            ServerVariable {
                enumeration: Some(vec!["us".to_owned()]),
                default_value: "eu".to_owned(),
                description: None,
                extensions: crate::model::extensions::Extensions::new(),
            },
        )];
        document
    });
    push(
        "InvalidExtensionName",
        document_with(&[(
            "/users",
            get(operation().with_parameter({
                let mut parameter = Parameter::query("q", Schema::of_type(SchemaType::String));
                parameter.extensions.insert("x-oai-reserved", true);
                parameter
            })),
        )]),
    );
    cases
}

/// Cases for the opacity rules, which are what make a document's authority
/// claim checkable rather than asserted.
fn ledger_opacity() -> Vec<(&'static str, SpecVersion, Document)> {
    use crate::model::schema::Schema;

    let operation = || Operation::new("listUsers").with_responses(ok_responses());
    let get = |operation: Operation| PathItem::new().with_operation(Method::Get, operation);

    let mut cases: Vec<(&'static str, SpecVersion, Document)> = Vec::new();
    let mut push =
        |name: &'static str, document: Document| cases.push((name, SpecVersion::V3_1, document));

    push("UncheckedSchema", {
        let mut unconstrained = Operation::new("ingest").with_responses(ok_responses());
        unconstrained.request_body =
            Some(crate::RefOr::Item(crate::RequestBody::json(Schema::any())));
        document_with(&[(
            "/ingest",
            PathItem::new().with_operation(Method::Post, unconstrained),
        )])
    });

    // Validating as 3.1 a document only 3.2 can express. The same walk
    // `Document::emit` refuses on, so the two agree on what 3.1 can carry.
    // `$self` is written to the wire and read back, so each build holds it
    // where it parses it: a 3.2-capable build in `Document::self_uri`, a
    // 3.1-only one in the root's extensions, where it is just as refused.
    push("RequiresV3_2", {
        let mut document = document_with(&[("/users", get(operation()))]);
        document
            .extensions
            .insert("$self", "https://example.com/orders");
        serde_json::from_value(serde_json::to_value(&document).expect("serializable"))
            .expect("a description carrying `$self` parses in every build")
    });

    // One opaque operation raises three at once: the operation is reported, the
    // document is no longer authoritative, and the stamp saying so is present.
    push("OpaqueOperation", opaque_document());
    push("NotAuthoritative", opaque_document());
    push("AuthorityNotStamped", {
        let mut document = opaque_document();
        document
            .extensions
            .0
            .shift_remove(crate::annotation::NOT_AUTHORITATIVE_ANNOTATION);
        document
    });
    push("StaleAuthorityStamp", {
        let mut document = document_with(&[("/users", get(operation()))]);
        document
            .extensions
            .insert(crate::annotation::NOT_AUTHORITATIVE_ANNOTATION, true);
        document
    });
    push("OpaqueRoute", {
        let mut document = document_with(&[("/users", get(operation()))]);
        crate::annotation::OpaqueRoute::new(
            "/assets/{*path}",
            crate::annotation::OpaqueReason::UntypedRoute,
        )
        .append_to(&mut document)
        .expect("nothing to conflict with");
        document.restamp_authority();
        document
    });
    push("MalformedAnnotation", {
        let mut document = document_with(&[("/users", get(operation()))]);
        document.extensions.insert(
            crate::annotation::OPAQUE_ROUTES_ANNOTATION,
            serde_json::json!("nonsense"),
        );
        document
    });
    push("CallbackKeyedAsExtension", {
        let mut document = document_with(&[]);
        document.components.callbacks.insert(
            "OnData".to_owned(),
            crate::RefOr::Item(
                crate::model::callback::Callback::new().with("x-hook", get(operation())),
            ),
        );
        document
    });

    cases
}

/// Cases for the Discriminator Object's MUSTs.
fn ledger_discriminator() -> Vec<(&'static str, SpecVersion, Document)> {
    let mut uncomposed = kinded(true);
    uncomposed.discriminator = Some(Discriminator::new("kind"));
    let optional =
        || document_with_schemas([("Pet", object(discriminated(vec![object(kinded(false))])))]);

    vec![
        (
            "DiscriminatorWithoutComposition",
            SpecVersion::V3_1,
            document_with_schemas([("Pet", object(uncomposed))]),
        ),
        (
            "DiscriminatorPropertyNotRequired",
            SpecVersion::V3_1,
            optional(),
        ),
        #[cfg(feature = "openapi32")]
        (
            "DiscriminatorWithoutDefaultMapping",
            SpecVersion::V3_2,
            optional(),
        ),
    ]
}

/// Each ledger entry raises the variant it is filed under.
#[test]
fn each_ledger_case_raises_the_variant_it_names() {
    for (expected, version, document) in ledger() {
        let raised: Vec<&'static str> = Validator::new(version)
            .validate(&document)
            .iter()
            .map(|violation| variant_name(&violation.error))
            .collect();
        assert!(
            raised.contains(&expected),
            "`{expected}` was not raised; got {raised:?}"
        );
    }
}

/// The name of each [`SpecError`] variant, read off `violation.rs`: the first
/// line indented as a variant after each `#[error` attribute, whatever `cfg`
/// stands between them.
fn declared_variants() -> BTreeSet<&'static str> {
    let mut declared = BTreeSet::new();
    let mut attributed = false;
    for line in include_str!("violation.rs").lines() {
        if line.starts_with("    #[error") {
            attributed = true;
        } else if attributed
            && let Some(variant) = line.strip_prefix("    ")
            && variant.starts_with(|c: char| c.is_ascii_uppercase())
        {
            let end = variant
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(variant.len());
            declared.insert(&variant[..end]);
            attributed = false;
        }
    }
    declared
}

/// Every variant the validator can raise has a ledger entry raising it.
///
/// The set comes from the source rather than from a second list, so a variant
/// added without a case fails here, by name, instead of being silently
/// uncovered. That is the same instrument as
/// `every_rejected_schema_type_has_a_case` in `crates/kynos/tests/ui.rs`,
/// pointed at this enum, and names the set as `docs/testing.md` asks wherever
/// the set has names.
#[test]
fn every_variant_has_a_case() {
    let declared = declared_variants();
    let elsewhere: BTreeSet<&str> = RAISED_ELSEWHERE.iter().copied().collect();
    assert_eq!(
        elsewhere.len(),
        RAISED_ELSEWHERE.len(),
        "`RAISED_ELSEWHERE` names a variant twice"
    );
    assert!(
        elsewhere.is_subset(&declared),
        "`RAISED_ELSEWHERE` names no variant: {:?}",
        elsewhere.difference(&declared).collect::<Vec<_>>()
    );

    let raised: BTreeSet<&'static str> = ledger()
        .iter()
        .flat_map(|(_, version, document)| Validator::new(*version).validate(document))
        .map(|violation| variant_name(&violation.error))
        .collect();

    assert_eq!(
        raised,
        &declared - &elsewhere,
        "the ledger raises exactly the variants not raised elsewhere"
    );
}

/// A media type name is a map key that always holds a `/`, and it lives under
/// `content`, which the Request Body Object requires.
///
/// Both halves are about the same thing: the location has to be a pointer a
/// caller can resolve to the offending object. `violation_locations_escape_their_path_keys`
/// covers the path key; nothing covered the media type, which is the other key
/// on the way down and the only one guaranteed to contain a separator.
#[test]
fn violation_locations_name_the_media_type_they_came_from() {
    use crate::model::body::{RequestBody, media_type::MediaType};

    let item = PathItem::new().with_operation(
        Method::Post,
        Operation::new("createOrder")
            .with_request_body(RequestBody::new(
                "application/json",
                // Unconstrained, so the schema rule reports it and the
                // location it reports is the thing under test.
                MediaType::new(Schema::any()),
            ))
            .with_responses(Responses::new().with(
                200,
                Response::with_content("ok", "application/json", MediaType::new(Schema::any())),
            )),
    );
    let found = violations(&document_with(&[("/orders", item)]));
    let located: Vec<&str> = found
        .iter()
        .filter(|violation| matches!(violation.error, SpecError::UncheckedSchema))
        .map(|violation| violation.location.as_str())
        .collect();

    assert!(
        located.contains(&"#/paths/~1orders/post/requestBody/content/application~1json/schema"),
        "a request body's media type: {located:?}"
    );
    assert!(
        located.contains(&"#/paths/~1orders/post/responses/200/content/application~1json/schema"),
        "a response's media type: {located:?}"
    );
}

// --- Unchecked schemas, wherever a document nests one -----------------------

/// A schema carrying the annotation `Unchecked` emits, and nothing else.
fn annotated() -> Schema {
    let mut object = crate::model::schema::object::SchemaObject::default();
    object.unknown_keywords.insert(
        crate::annotation::UNCHECKED_SCHEMA_ANNOTATION.to_owned(),
        true.into(),
    );
    Schema::Object(Box::new(object))
}

/// A document whose one operation reads `schema` as a JSON request body.
fn document_with_body(schema: Schema) -> Document {
    use crate::model::body::{RequestBody, media_type::MediaType};

    let item = PathItem::new().with_operation(
        Method::Post,
        Operation::new("ingest")
            .with_request_body(RequestBody::new("application/json", MediaType::new(schema)))
            .with_responses(ok_responses()),
    );
    document_with(&[("/ingest", item)])
}

/// Where each `UncheckedSchema` was reported, sorted.
///
/// Each location is also resolved against the document's own JSON, so a
/// pointer that names a keyword the wire does not spell fails here rather
/// than reading as a plausible string.
fn unchecked_locations(document: &Document) -> Vec<String> {
    let json = serde_json::to_value(document).expect("a document serializes");
    let mut located: Vec<String> = violations(document)
        .into_iter()
        .filter(|violation| violation.error == SpecError::UncheckedSchema)
        .map(|violation| violation.location)
        .collect();
    located.sort();

    for location in &located {
        let pointer = location.strip_prefix('#').expect("a fragment pointer");
        let target = json
            .pointer(pointer)
            .unwrap_or_else(|| panic!("{location} resolves to nothing in {json}"));
        assert!(
            target == &serde_json::Value::Bool(true)
                || target
                    .get(crate::annotation::UNCHECKED_SCHEMA_ANNOTATION)
                    .is_some(),
            "{location} resolves to {target}, which is not unchecked"
        );
    }
    located
}

/// The `SchemaObject` fields whose type holds a `Schema`, read off its source.
///
/// Read rather than transcribed, so a subschema keyword added to the model
/// fails [`an_unchecked_schema_is_reported_under_every_subschema_keyword`]
/// until the walk and its table both reach it.
fn schema_bearing_fields() -> Vec<String> {
    let source = include_str!("../model/schema/object.rs");
    let body = source
        .split_once("pub struct SchemaObject {")
        .and_then(|(_, rest)| rest.split_once("\n}"))
        .map(|(body, _)| body)
        .expect("`SchemaObject` is declared in object.rs");

    let mut fields = Vec::new();
    let mut declaration = String::new();
    for line in body.lines().map(str::trim) {
        if declaration.is_empty() && !line.starts_with("pub ") {
            continue;
        }
        declaration.push_str(line);
        if !line.ends_with(',') {
            continue;
        }
        let (name, ty) = declaration["pub ".len()..]
            .split_once(':')
            .expect("a field declaration names its type");
        if ty
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|token| token == "Schema")
        {
            fields.push(name.trim().to_owned());
        }
        declaration.clear();
    }
    fields.sort();
    fields
}

/// Every JSON Schema keyword that holds a subschema is walked.
///
/// A derived body puts an `Unchecked` field under `properties`, an `Option` of
/// one under `anyOf`, and a `Vec` of one under `items`, and each used to raise
/// nothing: only a media type's own schema was read. The table is closed over
/// the model's fields, so the walk cannot quietly skip a keyword added later.
#[test]
fn an_unchecked_schema_is_reported_under_every_subschema_keyword() {
    use crate::model::schema::object::SchemaObject;

    type Place = fn(&mut SchemaObject, Schema);
    let table: &[(&str, &str, Place)] = &[
        ("defs", "$defs/Inner", |o, s| {
            o.defs.insert("Inner".to_owned(), s);
        }),
        ("all_of", "allOf/1", |o, s| {
            o.all_of = Some(vec![Schema::of_type(SchemaType::Object), s]);
        }),
        ("any_of", "anyOf/0", |o, s| {
            o.any_of = Some(vec![s, Schema::of_type(SchemaType::Null)]);
        }),
        ("one_of", "oneOf/0", |o, s| o.one_of = Some(vec![s])),
        ("not", "not", |o, s| o.not = Some(Box::new(s))),
        ("if_schema", "if", |o, s| o.if_schema = Some(Box::new(s))),
        ("then_schema", "then", |o, s| {
            o.then_schema = Some(Box::new(s));
        }),
        ("else_schema", "else", |o, s| {
            o.else_schema = Some(Box::new(s));
        }),
        ("dependent_schemas", "dependentSchemas/extra", |o, s| {
            o.dependent_schemas.insert("extra".to_owned(), s);
        }),
        ("prefix_items", "prefixItems/0", |o, s| {
            o.prefix_items = Some(vec![s]);
        }),
        ("items", "items", |o, s| o.items = Some(Box::new(s))),
        ("contains", "contains", |o, s| {
            o.contains = Some(Box::new(s));
        }),
        ("properties", "properties/payload", |o, s| {
            o.properties.insert("payload".to_owned(), s);
        }),
        // A key holding a `/`, so the pointer has to escape it.
        ("pattern_properties", "patternProperties/^a~1b$", |o, s| {
            o.pattern_properties.insert("^a/b$".to_owned(), s);
        }),
        ("additional_properties", "additionalProperties", |o, s| {
            o.additional_properties = Some(Box::new(s));
        }),
        ("property_names", "propertyNames", |o, s| {
            o.property_names = Some(Box::new(s));
        }),
        ("unevaluated_items", "unevaluatedItems", |o, s| {
            o.unevaluated_items = Some(Box::new(s));
        }),
        ("unevaluated_properties", "unevaluatedProperties", |o, s| {
            o.unevaluated_properties = Some(Box::new(s));
        }),
        ("content_schema", "contentSchema", |o, s| {
            o.content_schema = Some(Box::new(s));
        }),
    ];

    let mut tabled: Vec<String> = table
        .iter()
        .map(|(field, ..)| (*field).to_owned())
        .collect();
    tabled.sort();
    assert_eq!(tabled, schema_bearing_fields(), "the table is closed");

    for (field, suffix, place) in table {
        let mut parent = SchemaObject::default();
        place(&mut parent, annotated());

        assert_eq!(
            unchecked_locations(&document_with_body(Schema::Object(Box::new(parent)))),
            [format!(
                "#/paths/~1ingest/post/requestBody/content/application~1json/schema/{suffix}"
            )],
            "{field}"
        );
    }
}

/// A component is reported where it is written, not once per `$ref` to it.
///
/// This is the shape a derived body takes once it is registered: the media
/// type holds a `$ref`, and the annotated field sits under the component. A
/// reference is never followed, so two operations naming one component still
/// raise one violation.
#[test]
fn a_referenced_component_is_reported_once_where_it_is_defined() {
    use crate::model::{body::media_type::MediaType, schema::object::SchemaObject};

    let mut feed = SchemaObject::default();
    feed.properties.insert("payload".to_owned(), annotated());

    let mut document = document_with_body(Schema::component("Feed"));
    if let Some(PathItem {
        post: Some(operation),
        ..
    }) = document.paths.items.get_mut("/ingest")
    {
        operation.responses = Responses::new().with(
            200,
            Response::with_content(
                "ok",
                "application/json",
                MediaType::new(Schema::component("Feed")),
            ),
        );
    }
    document
        .components
        .schemas
        .insert("Feed".to_owned(), Schema::Object(Box::new(feed)));

    assert_eq!(
        unchecked_locations(&document),
        ["#/components/schemas/Feed/properties/payload"]
    );
}

/// A permissive schema is only unchecked where it *is* the payload.
///
/// `true` as a media type's own schema says the body is unconstrained. Below
/// that it is an ordinary keyword value: every problem document Kynos emits
/// carries `additionalProperties: true`, and reporting that would make
/// `deny_unchecked_schemas` refuse every router with a JSON body. The
/// annotation is what an author writes on purpose, and counts anywhere.
#[test]
fn a_permissive_schema_below_a_media_type_is_not_unchecked() {
    use crate::model::schema::object::SchemaObject;

    let open = SchemaObject {
        additional_properties: Some(Box::new(Schema::any())),
        properties: [("anything".to_owned(), Schema::any())]
            .into_iter()
            .collect(),
        ..SchemaObject::default()
    };

    let mut document = document_with_body(Schema::Object(Box::new(open)));
    document
        .components
        .schemas
        .insert("Anything".to_owned(), Schema::any());

    assert_eq!(unchecked_locations(&document), Vec::<String>::new());
}

/// A header described by `content` is walked into its media type.
///
/// The generator gives a content-form header an empty media type, so no
/// property run puts a schema there. Both places a header is written are
/// held: under a response, and as a reusable component.
#[test]
fn an_unchecked_schema_under_a_header_content_is_reported() {
    use crate::model::{body::media_type::MediaType, parameter::header::Header, reference::RefOr};

    let header = || Header::with_content("application/json", MediaType::new(annotated()));

    let mut document = document_with_body(Schema::of_type(SchemaType::Object));
    if let Some(PathItem {
        post: Some(operation),
        ..
    }) = document.paths.items.get_mut("/ingest")
    {
        let mut response = Response::new("ok");
        response
            .headers
            .insert("X-Trace".to_owned(), RefOr::Item(header()));
        operation.responses = Responses::new().with(200, response);
    }
    document
        .components
        .headers
        .insert("Trace".to_owned(), RefOr::Item(header()));

    assert_eq!(
        unchecked_locations(&document),
        [
            "#/components/headers/Trace/content/application~1json/schema",
            "#/paths/~1ingest/post/responses/200/headers/X-Trace/content/application~1json/schema",
        ]
    );
}

/// The containers a generated document does not reach are walked too.
///
/// The property in `tests/properties.rs` covers every container its generator
/// draws. It leaves a media type's `itemEncoding` and an encoding's own nested
/// encodings empty, so those positions are held here.
#[cfg(feature = "openapi32")]
#[test]
fn an_unchecked_schema_under_a_nested_encoding_is_reported() {
    use crate::model::{
        body::{encoding::Encoding, media_type::MediaType},
        parameter::header::Header,
        reference::RefOr,
    };

    let with_header = |schema: Schema| {
        let mut encoding = Encoding::default();
        encoding
            .headers
            .insert("X-Part".to_owned(), RefOr::Item(Header::new(schema)));
        encoding
    };

    let mut outer = Encoding::default();
    outer
        .encoding
        .insert("inner".to_owned(), with_header(annotated()));
    outer.prefix_encoding = Some(vec![with_header(annotated())]);
    outer.item_encoding = Some(Box::new(with_header(annotated())));

    let mut media_type = MediaType::new(Schema::of_type(SchemaType::Array));
    media_type.item_encoding = Some(Box::new(outer));
    media_type.item_schema = Some(annotated());

    let mut document = document_with(&[]);
    document
        .components
        .media_types
        .insert("Parts".to_owned(), RefOr::Item(media_type));

    assert_eq!(
        unchecked_locations(&document),
        [
            "#/components/mediaTypes/Parts/itemEncoding/encoding/inner/headers/X-Part/schema",
            "#/components/mediaTypes/Parts/itemEncoding/itemEncoding/headers/X-Part/schema",
            "#/components/mediaTypes/Parts/itemEncoding/prefixEncoding/0/headers/X-Part/schema",
            "#/components/mediaTypes/Parts/itemSchema",
        ]
    );
}

// --- The Discriminator Object's MUSTs ---------------------------------------

/// An object with a string `kind` property, which it requires when `required`.
fn kinded(required: bool) -> SchemaObject {
    let mut object = SchemaObject {
        ty: Some(TypeSet::One(SchemaType::Object)),
        ..SchemaObject::default()
    };
    object
        .properties
        .insert("kind".to_owned(), Schema::of_type(SchemaType::String));
    if required {
        object.required = Some(vec!["kind".to_owned()]);
    }
    object
}

/// A `oneOf` over `branches`, discriminated by `kind`.
fn discriminated(branches: Vec<Schema>) -> SchemaObject {
    SchemaObject {
        one_of: Some(branches),
        discriminator: Some(Discriminator::new("kind")),
        ..SchemaObject::default()
    }
}

fn object(object: SchemaObject) -> Schema {
    Schema::Object(Box::new(object))
}

/// A document holding each of `schemas` under `#/components/schemas`.
fn document_with_schemas<'a>(schemas: impl IntoIterator<Item = (&'a str, Schema)>) -> Document {
    let mut document = document_with(&[]);
    for (name, schema) in schemas {
        document.components.schemas.insert(name.to_owned(), schema);
    }
    document
}

/// Each discriminator violation validating as `version` raises, at its
/// location.
fn discriminator_errors(document: &Document, version: SpecVersion) -> Vec<(String, &'static str)> {
    Validator::new(version)
        .validate(document)
        .into_iter()
        .map(|violation| (violation.location, variant_name(&violation.error)))
        .filter(|(_, name)| name.starts_with("Discriminator"))
        .collect()
}

/// Every version a build validates against.
fn versions() -> Vec<SpecVersion> {
    vec![
        SpecVersion::V3_1,
        #[cfg(feature = "openapi32")]
        SpecVersion::V3_2,
    ]
}

/// The Discriminator Object is legal only beside `oneOf`, `anyOf` or `allOf`
/// (`references/3.1.2.md:3341`, `references/3.2.0.md:3623`).
#[test]
fn a_discriminator_beside_no_composition_is_refused() {
    let mut pet = kinded(true);
    pet.discriminator = Some(Discriminator::new("kind"));
    let document = document_with_schemas([("Pet", object(pet))]);

    for version in versions() {
        assert_eq!(
            discriminator_errors(&document, version),
            [(
                "#/components/schemas/Pet/discriminator".to_owned(),
                "DiscriminatorWithoutComposition"
            )],
            "validating as {version}"
        );
    }
}

/// The `allOf` form puts the discriminator on a parent its children extend
/// through `allOf` (`references/3.1.2.md:3345`, the specification's own
/// `Pet` example), so the parent needs no composition of its own.
#[test]
fn the_parent_an_all_of_member_references_needs_no_composition() {
    let mut pet = kinded(true);
    pet.discriminator = Some(Discriminator::new("kind"));
    let cat = SchemaObject {
        all_of: Some(vec![
            Schema::component("Pet"),
            Schema::of_type(SchemaType::Object),
        ]),
        ..SchemaObject::default()
    };
    let document = document_with_schemas([("Pet", object(pet)), ("Cat", object(cat))]);

    for version in versions() {
        let errors = discriminator_errors(&document, version);
        assert!(errors.is_empty(), "validating as {version}: {errors:?}");
    }
}

/// 3.1 makes the discriminating property required (`references/3.1.2.md:2834`),
/// so one `oneOf` branch leaving it optional is enough to refuse it.
#[test]
fn a_discriminating_property_must_be_required_under_three_one() {
    let pet = discriminated(vec![object(kinded(true)), object(kinded(false))]);
    let document = document_with_schemas([("Pet", object(pet))]);

    assert_eq!(
        discriminator_errors(&document, SpecVersion::V3_1),
        [(
            "#/components/schemas/Pet/discriminator".to_owned(),
            "DiscriminatorPropertyNotRequired"
        )]
    );
}

/// 3.2 lets the discriminating property be optional only beside a
/// `defaultMapping` (`references/3.2.0.md:3615`).
#[cfg(feature = "openapi32")]
#[test]
fn an_optional_discriminating_property_needs_a_default_mapping_under_three_two() {
    let mut pet = discriminated(vec![object(kinded(true)), object(kinded(false))]);
    let document = document_with_schemas([("Pet", object(pet.clone()))]);
    assert_eq!(
        discriminator_errors(&document, SpecVersion::V3_2),
        [(
            "#/components/schemas/Pet/discriminator".to_owned(),
            "DiscriminatorWithoutDefaultMapping"
        )]
    );

    pet.discriminator
        .as_mut()
        .expect("discriminated")
        .default_mapping = Some("#/components/schemas/Pet".to_owned());
    let document = document_with_schemas([("Pet", object(pet))]);
    let errors = discriminator_errors(&document, SpecVersion::V3_2);
    assert!(errors.is_empty(), "{errors:?}");
}

/// Whether the property is required is read through what composes the
/// schema: an `allOf` member, a `#/components/schemas` reference, both arms of
/// a conditional, and a `false` branch that admits nothing. A reference the
/// rule cannot follow, or one it is already following, is not judged.
#[test]
fn a_discriminating_property_is_required_through_what_composes_the_schema() {
    let conditional = SchemaObject {
        if_schema: Some(Box::new(Schema::of_type(SchemaType::Object))),
        then_schema: Some(Box::new(object(kinded(true)))),
        else_schema: Some(Box::new(object(kinded(true)))),
        ..SchemaObject::default()
    };
    let conjunction = SchemaObject {
        all_of: Some(vec![
            Schema::of_type(SchemaType::Object),
            object(kinded(true)),
        ]),
        ..SchemaObject::default()
    };
    let pet = discriminated(vec![
        Schema::component("Cat"),
        object(conditional),
        object(conjunction),
        Schema::reference("https://example.com/lizard.json"),
        Schema::never(),
    ]);
    let document = document_with_schemas([
        ("Pet", object(pet)),
        ("Cat", object(kinded(true))),
        (
            "Loop",
            object(discriminated(vec![Schema::component("Loop")])),
        ),
    ]);
    for version in versions() {
        let errors = discriminator_errors(&document, version);
        assert!(errors.is_empty(), "validating as {version}: {errors:?}");
    }

    // Followed, not trusted: a component leaving the property optional leaves
    // it optional.
    let pet = discriminated(vec![Schema::component("Dog")]);
    let document = document_with_schemas([("Pet", object(pet)), ("Dog", object(kinded(false)))]);
    assert_eq!(
        discriminator_errors(&document, SpecVersion::V3_1),
        [(
            "#/components/schemas/Pet/discriminator".to_owned(),
            "DiscriminatorPropertyNotRequired"
        )]
    );

    // A `$dynamicRef` resolves against the dynamic scope, which could require
    // the property unseen.
    let dynamic = SchemaObject {
        dynamic_ref: Some("#node".to_owned()),
        ..SchemaObject::default()
    };
    assert_eq!(single_branch_errors(dynamic), Vec::<&str>::new());

    // An `anyOf` requires the property when every branch does, and only then.
    let every = SchemaObject {
        any_of: Some(vec![object(kinded(true)), object(kinded(true))]),
        ..SchemaObject::default()
    };
    assert_eq!(single_branch_errors(every), Vec::<&str>::new());
    let some = SchemaObject {
        any_of: Some(vec![object(kinded(true)), object(kinded(false))]),
        ..SchemaObject::default()
    };
    assert_eq!(single_branch_errors(some), [NOT_REQUIRED]);

    // A conditional requires it only through both arms.
    let one_armed = SchemaObject {
        if_schema: Some(Box::new(Schema::of_type(SchemaType::Object))),
        then_schema: Some(Box::new(object(kinded(true)))),
        else_schema: Some(Box::new(object(kinded(false)))),
        ..SchemaObject::default()
    };
    assert_eq!(single_branch_errors(one_armed), [NOT_REQUIRED]);

    // `not`, `dependentSchemas` and `dependentRequired` can each require the
    // property in a way the rule does not read, so each gets the benefit of
    // the doubt rather than a false positive.
    let negated = SchemaObject {
        not: Some(Box::new(Schema::Object(Box::new(SchemaObject {
            not: Some(Box::new(object(kinded(true)))),
            ..SchemaObject::default()
        })))),
        ..SchemaObject::default()
    };
    assert_eq!(single_branch_errors(negated), Vec::<&str>::new());
    let mut dependent = SchemaObject {
        required: Some(vec!["a".to_owned()]),
        ..SchemaObject::default()
    };
    dependent
        .dependent_schemas
        .insert("a".to_owned(), object(kinded(true)));
    assert_eq!(single_branch_errors(dependent), Vec::<&str>::new());
    let mut dependent = SchemaObject {
        required: Some(vec!["a".to_owned()]),
        ..SchemaObject::default()
    };
    dependent
        .dependent_required
        .insert("a".to_owned(), vec!["kind".to_owned()]);
    assert_eq!(single_branch_errors(dependent), Vec::<&str>::new());
}

/// Each component a discriminator's requiredness reaches is followed once, so
/// a chain whose every link references the next twice is judged in linear
/// rather than exponential time: at 64 links the latter never finishes, and
/// nextest's bound reports it.
#[test]
fn a_component_reached_twice_is_followed_once() {
    const LINKS: usize = 64;
    let link = |index: usize| format!("Link{index}");
    let mut schemas: Vec<(String, Schema)> = (0..LINKS)
        .map(|index| {
            let next = Schema::component(&link(index + 1));
            let doubled = SchemaObject {
                all_of: Some(vec![next.clone(), next]),
                ..SchemaObject::default()
            };
            (link(index), object(doubled))
        })
        .collect();
    schemas.push((link(LINKS), object(kinded(false))));
    schemas.push((
        "Pet".to_owned(),
        object(discriminated(vec![Schema::component(&link(0))])),
    ));
    let document = document_with_schemas(
        schemas
            .iter()
            .map(|(name, schema)| (name.as_str(), schema.clone())),
    );

    assert_eq!(
        discriminator_errors(&document, SpecVersion::V3_1),
        [(
            "#/components/schemas/Pet/discriminator".to_owned(),
            NOT_REQUIRED
        )]
    );
}

/// A verdict reached by assuming a component still being followed requires
/// the property is not remembered past that component: `B` counts as
/// requiring while `A` is followed, but `A` turns out optional, so `B`, and
/// `Pet` through both, is optional.
#[test]
fn a_verdict_reached_through_a_cut_cycle_is_not_remembered() {
    let pet = SchemaObject {
        all_of: Some(vec![Schema::component("A"), Schema::component("B")]),
        discriminator: Some(Discriminator::new("kind")),
        ..SchemaObject::default()
    };
    let a = SchemaObject {
        any_of: Some(vec![Schema::component("B"), Schema::Bool(true)]),
        ..SchemaObject::default()
    };
    let b = SchemaObject {
        all_of: Some(vec![Schema::component("A")]),
        ..SchemaObject::default()
    };
    let document =
        document_with_schemas([("Pet", object(pet)), ("A", object(a)), ("B", object(b))]);

    assert_eq!(
        discriminator_errors(&document, SpecVersion::V3_1),
        [(
            "#/components/schemas/Pet/discriminator".to_owned(),
            NOT_REQUIRED
        )]
    );
}

/// A verdict reached by assuming a component still being followed is reused
/// while that component is followed, so a chain whose every link references
/// the next twice and whose last link closes the cycle is judged in linear
/// rather than exponential time: at 64 links the latter never finishes, and
/// nextest's bound reports it.
#[test]
fn a_verdict_reached_through_a_cut_cycle_is_reused_within_it() {
    const LINKS: usize = 64;
    let link = |index: usize| format!("Link{index}");
    let mut schemas: Vec<(String, Schema)> = (0..LINKS)
        .map(|index| {
            let next = Schema::component(&link(index + 1));
            let doubled = SchemaObject {
                any_of: Some(vec![next.clone(), next]),
                ..SchemaObject::default()
            };
            (link(index), object(doubled))
        })
        .collect();
    let closing = SchemaObject {
        all_of: Some(vec![Schema::component(&link(0))]),
        ..SchemaObject::default()
    };
    schemas.push((link(LINKS), object(closing)));
    schemas.push((
        "Pet".to_owned(),
        object(discriminated(vec![Schema::component(&link(0))])),
    ));
    let document = document_with_schemas(
        schemas
            .iter()
            .map(|(name, schema)| (name.as_str(), schema.clone())),
    );

    assert_eq!(
        discriminator_errors(&document, SpecVersion::V3_1),
        Vec::<(String, &str)>::new()
    );
}

/// The discriminator violations validating, as 3.1, a `Pet` discriminated
/// over `branch` alone raises, each at its `discriminator`.
fn single_branch_errors(branch: SchemaObject) -> Vec<&'static str> {
    let pet = discriminated(vec![object(branch)]);
    discriminator_errors(
        &document_with_schemas([("Pet", object(pet))]),
        SpecVersion::V3_1,
    )
    .into_iter()
    .map(|(location, name)| {
        assert_eq!(location, "#/components/schemas/Pet/discriminator");
        name
    })
    .collect()
}

/// `Pet`'s discriminating property reported optional under 3.1.
const NOT_REQUIRED: &str = "DiscriminatorPropertyNotRequired";

/// What the `Schema` derive writes for a tagged enum, transcribed from
/// `kynos-macros/src/derive/schema/shape.rs`, is clean: a `oneOf` of inline
/// branches each requiring the tag, a newtype variant's through an `allOf`
/// with a tag-only marker, discriminated by the tag with no `mapping`.
#[test]
fn the_schema_derives_tagged_enums_are_clean() {
    let unit = kinded(true);
    let mut named = kinded(true);
    named
        .properties
        .insert("name".to_owned(), Schema::of_type(SchemaType::String));
    let newtype = SchemaObject {
        all_of: Some(vec![object(kinded(true)), Schema::component("Payload")]),
        ..SchemaObject::default()
    };
    let internally = discriminated(vec![object(unit), object(named), object(newtype)]);

    let mut adjacent = kinded(true);
    adjacent
        .properties
        .insert("content".to_owned(), Schema::component("Payload"));
    adjacent
        .required
        .get_or_insert_with(Vec::new)
        .push("content".to_owned());
    let adjacently = discriminated(vec![object(kinded(true)), object(adjacent)]);

    let document = document_with_schemas([
        ("Internally", object(internally)),
        ("Adjacently", object(adjacently)),
        ("Payload", Schema::of_type(SchemaType::Object)),
    ]);
    for version in versions() {
        let errors = discriminator_errors(&document, version);
        assert!(errors.is_empty(), "validating as {version}: {errors:?}");
    }
}
