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
