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
        MediaType::new(Schema::of_type(SchemaType::Object)).with_encoding("part", encoding),
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

#[test]
fn an_operation_must_declare_a_response() {
    let item = PathItem::new().with_operation(Method::Get, Operation::new("listUsers"));
    let found = errors(&document_with(&[("/users", item)]));
    assert!(matches!(found.as_slice(), [SpecError::NoResponses]));
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
        SpecError::InvalidPathTemplate { .. } => "InvalidPathTemplate",
        SpecError::UndeclaredPathVariable { .. } => "UndeclaredPathVariable",
        SpecError::UnusedPathParameter { .. } => "UnusedPathParameter",
        SpecError::PathParameterNotRequired { .. } => "PathParameterNotRequired",
        SpecError::DuplicateParameter { .. } => "DuplicateParameter",
        SpecError::ShortCircuitMismatch { .. } => "ShortCircuitMismatch",
        SpecError::IllegalStyle { .. } => "IllegalStyle",
        SpecError::IgnoredHeaderParameter { .. } => "IgnoredHeaderParameter",
        SpecError::IgnoredHeader { .. } => "IgnoredHeader",
        SpecError::NoResponses => "NoResponses",
        SpecError::MissingResponseDescription => "MissingResponseDescription",
        SpecError::InvalidComponentName { .. } => "InvalidComponentName",
        SpecError::DuplicateTag { .. } => "DuplicateTag",
        SpecError::UnknownTagParent { .. } => "UnknownTagParent",
        SpecError::TagParentCycle { .. } => "TagParentCycle",
        SpecError::UnknownSecurityScheme { .. } => "UnknownSecurityScheme",
        SpecError::UndocumentedTag { .. } => "UndocumentedTag",
        SpecError::EmptyServerVariableEnum { .. } => "EmptyServerVariableEnum",
        SpecError::ServerVariableDefaultNotInEnum { .. } => "ServerVariableDefaultNotInEnum",
        SpecError::InvalidExtensionName { .. } => "InvalidExtensionName",
        SpecError::UncheckedSchema => "UncheckedSchema",
        SpecError::NotAuthoritative => "NotAuthoritative",
        SpecError::OpaqueOperation { .. } => "OpaqueOperation",
        SpecError::OpaqueRoute { .. } => "OpaqueRoute",
        SpecError::AuthorityNotStamped => "AuthorityNotStamped",
        SpecError::MalformedAnnotation { .. } => "MalformedAnnotation",
        SpecError::EmptyDocument => "EmptyDocument",
        SpecError::RequiresV3_2 { .. } => "RequiresV3_2",
    }
}

/// Variants no validation run raises, and what raises each instead.
///
/// Every one of these is reachable — none is a placeholder. Listing them is
/// what keeps [`every_variant_has_a_case`] a statement about this validator
/// rather than about the enum.
const RAISED_ELSEWHERE: &[&str] = &[
    // Prevented by construction rather than reported: `Document::paths` is
    // always serialized, so every emitted document carries one of the three
    // fields the rule asks for. The variant stays because a consumer
    // validating a description Kynos did not write still needs to name the
    // failure, and because `paths` being absent and `paths` being present but
    // empty are the same value here -- only the wire tells them apart.
    "EmptyDocument",
    // A 3.1-only build has no 3.2 construct to raise this with. A 3.2-capable
    // one does, and carries a ledger case below.
    #[cfg(not(feature = "openapi32"))]
    "RequiresV3_2",
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
];

/// One document per variant, each named by the variant it must raise and the
/// version to validate it at.
///
/// Split by the rule module that raises each group, which is also the order
/// `Validator::validate` runs them in.
fn ledger() -> Vec<(&'static str, SpecVersion, Document)> {
    let mut cases = ledger_paths();
    cases.extend(ledger_parameters());
    cases.extend(ledger_document());
    cases.extend(ledger_opacity());
    cases
}

/// Cases for the path, template and parameter rules.
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
    cases
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
                parameter.extensions.insert("not-prefixed", true);
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
    #[cfg(feature = "openapi32")]
    push("RequiresV3_2", {
        let mut document = document_with(&[("/users", get(operation()))]);
        document.self_uri = Some("https://example.com/orders".to_owned());
        document
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

    cases
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

/// Every variant the validator can raise has a ledger entry raising it.
///
/// The count comes from the source rather than from a second list, so a variant
/// added without a case fails here instead of being silently uncovered. That is
/// the same instrument as `every_rejected_schema_type_has_a_case` in
/// `crates/kynos/tests/ui.rs`, pointed at this enum.
#[test]
fn every_variant_has_a_case() {
    let declared = include_str!("violation.rs")
        .matches("\n    #[error")
        .count();

    let mut raised: Vec<&'static str> = ledger()
        .iter()
        .flat_map(|(_, version, document)| Validator::new(*version).validate(document))
        .map(|violation| variant_name(&violation.error))
        .collect();
    raised.sort_unstable();
    raised.dedup();

    let uncovered = declared - RAISED_ELSEWHERE.len();
    assert_eq!(
        raised.len(),
        uncovered,
        "{declared} variants declared, {} raised elsewhere, {} covered here: {raised:?}",
        RAISED_ELSEWHERE.len(),
        raised.len()
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
                // Unconstrained, so `check_media_type` reports it and the
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
        located.contains(&"#/paths/~1orders/post/requestBody/content/application~1json"),
        "a request body's media type: {located:?}"
    );
    assert!(
        located.contains(&"#/paths/~1orders/post/responses/200/content/application~1json"),
        "a response's media type: {located:?}"
    );
}
