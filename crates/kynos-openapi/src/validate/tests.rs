use super::{SpecError, Validator, Violation};
use crate::model::{
    document::{Document, SpecVersion},
    info::Info,
    parameter::{Parameter, style::Style},
    paths::{item::PathItem, method::Method, operation::Operation, template::PathTemplate},
    response::{Response, Responses},
    schema::{Schema, types::SchemaType},
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
fn a_parameter_must_set_exactly_one_of_schema_and_content() {
    let mut parameter = Parameter::query("filter", Schema::any());
    parameter.schema = None;

    let item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("listUsers")
            .with_parameter(parameter)
            .with_responses(ok_responses()),
    );

    let found = errors(&document_with(&[("/users", item)]));
    assert!(matches!(
        found.as_slice(),
        [SpecError::SchemaContentExclusivity { .. }]
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
fn an_operation_must_declare_a_response() {
    let item = PathItem::new().with_operation(Method::Get, Operation::new("listUsers"));
    let found = errors(&document_with(&[("/users", item)]));
    assert!(matches!(found.as_slice(), [SpecError::NoResponses]));
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
fn a_conflicting_license_is_rejected() {
    let mut document = document_with(&[]);
    let mut license = crate::License::spdx("MIT", "MIT");
    license.url = Some("https://example.com".to_owned());
    document.info.license = Some(license);

    assert!(
        errors(&document)
            .iter()
            .any(|e| matches!(e, SpecError::LicenseExclusivity))
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
    assert_eq!(document.paths.0.len(), 1);
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
            | SpecError::OpaqueOperation { .. }
            | SpecError::OpaqueRoute { .. }
    )));
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
