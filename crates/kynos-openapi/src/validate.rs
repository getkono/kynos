//! Structural validation of a [`Document`].
//!
//! Everything checked here is a rule the OpenAPI specification states but that
//! the type system cannot enforce on its own — uniqueness across a whole
//! document, correspondence between a path template and its parameters, mutual
//! exclusions between optional fields.
//!
//! Kynos runs this when a router is built, so a description that would mislead
//! a client generator fails at startup rather than being published.

use std::collections::{HashMap, HashSet};

use crate::{
    Map,
    body::MediaType,
    components::ComponentName,
    document::{Document, SpecVersion},
    extensions::Extensions,
    parameter::{Header, Parameter, ParameterIn, is_ignored_header_parameter},
    paths::{Operation, PathItem, PathTemplate},
    reference::RefOr,
};

/// The annotation marking a schema as deliberately unconstrained.
///
/// Kynos attaches this wherever a handler used the explicit permissive type, so
/// that "this payload is unchecked" is visible in the published description
/// rather than only in the Rust source.
pub const UNCHECKED_SCHEMA_ANNOTATION: &str = "x-kynos-unchecked";

/// The annotation marking a description as not fully describing the service.
///
/// Kynos attaches this when a router reached something it cannot describe — an
/// opaque layer, a wildcard route, a protocol upgrade — so that consumers know
/// the description is incomplete.
pub const NOT_AUTHORITATIVE_ANNOTATION: &str = "x-kynos-document-not-authoritative";

/// How seriously to take a [`Violation`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The description is invalid, or would mislead a consumer.
    Error,
    /// The description is valid but weaker than it could be.
    Warning,
}

/// A single problem found in a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    /// A JSON-Pointer-like path to the offending object.
    pub location: String,
    /// How seriously to take it.
    pub severity: Severity,
    /// What is wrong.
    pub error: SpecError,
}

impl Violation {
    fn error(location: impl Into<String>, error: SpecError) -> Self {
        Self {
            location: location.into(),
            severity: Severity::Error,
            error,
        }
    }

    fn warning(location: impl Into<String>, error: SpecError) -> Self {
        Self {
            location: location.into(),
            severity: Severity::Warning,
            error,
        }
    }
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "{label} at {}: {}", self.location, self.error)
    }
}

/// A way in which a document fails to conform.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SpecError {
    /// The same `operationId` was used more than once.
    #[error("`operationId` `{operation_id}` is not unique; it is also used at {first}")]
    DuplicateOperationId {
        /// The repeated identifier.
        operation_id: String,
        /// Where it was first seen.
        first: String,
    },

    /// Two path templates differ only in the names of their variables.
    #[error(
        "path `{template}` is the same path as `{existing}`; templates that differ \
         only in variable name are identical"
    )]
    DuplicatePathTemplate {
        /// The template being added.
        template: String,
        /// The template already present.
        existing: String,
    },

    /// A path template variable has no corresponding parameter.
    #[error("path template variable `{name}` has no matching `in: path` parameter")]
    UndeclaredPathVariable {
        /// The variable named in the template.
        name: String,
    },

    /// A path parameter does not appear in the path template.
    #[error("`in: path` parameter `{name}` does not appear in the path template")]
    UnusedPathParameter {
        /// The declared parameter.
        name: String,
    },

    /// A path parameter was not marked required.
    #[error("`in: path` parameter `{name}` must set `required: true`")]
    PathParameterNotRequired {
        /// The offending parameter.
        name: String,
    },

    /// Two parameters share a name and location.
    #[error("parameter `{name}` in `{location}` is declared more than once")]
    DuplicateParameter {
        /// The parameter name.
        name: String,
        /// Where it is carried.
        location: String,
    },

    /// A parameter or header set neither or both of `schema` and `content`.
    #[error("`{name}` must set exactly one of `schema` and `content`")]
    SchemaContentExclusivity {
        /// The parameter or header name.
        name: String,
    },

    /// A `content` map did not hold exactly one entry.
    #[error("the `content` of `{name}` must hold exactly one entry, found {found}")]
    ContentNotSingular {
        /// The parameter or header name.
        name: String,
        /// How many entries were present.
        found: usize,
    },

    /// A serialization style is not legal at a parameter's location.
    #[error("style `{style}` may not be used with `in: {location}`")]
    IllegalStyle {
        /// The style used.
        style: String,
        /// The location it was used at.
        location: String,
    },

    /// A header was declared as a parameter despite being ignored.
    #[error(
        "`{name}` must not be declared as a header parameter: the specification says \
         such a definition is ignored, so declaring it misdescribes the API"
    )]
    IgnoredHeaderParameter {
        /// The offending header name.
        name: String,
    },

    /// Both the singular and plural example fields were set.
    #[error("`example` and `examples` are mutually exclusive")]
    ExampleExclusivity,

    /// An Example Object set more than one of its value fields.
    #[error("an Example Object must set only one value field, found {found}")]
    ExampleValueExclusivity {
        /// How many value fields were set.
        found: usize,
    },

    /// A License Object set both `identifier` and `url`.
    #[error("`identifier` and `url` are mutually exclusive on a License Object")]
    LicenseExclusivity,

    /// A Link Object did not identify exactly one target operation.
    #[error("a Link Object must set exactly one of `operationRef` and `operationId`")]
    LinkTargetExclusivity,

    /// An operation declared no responses.
    #[error("an operation must declare at least one response")]
    NoResponses,

    /// A component key used characters the specification forbids.
    #[error("`{name}` is not a valid component name: expected only `A-Z a-z 0-9 . - _`")]
    InvalidComponentName {
        /// The offending key.
        name: String,
    },

    /// Two tags share a name.
    #[error("tag `{name}` is declared more than once")]
    DuplicateTag {
        /// The repeated tag name.
        name: String,
    },

    /// A tag named a parent that does not exist.
    #[error("tag `{name}` names parent `{parent}`, which is not declared")]
    UnknownTagParent {
        /// The tag with the parent.
        name: String,
        /// The parent that does not exist.
        parent: String,
    },

    /// A chain of tag parents forms a cycle.
    #[error("tag `{name}` is part of a parent cycle")]
    TagParentCycle {
        /// A tag on the cycle.
        name: String,
    },

    /// A security requirement named a scheme that is not declared.
    #[error("security requirement `{name}` does not name a declared security scheme")]
    UnknownSecurityScheme {
        /// The name used in the requirement.
        name: String,
    },

    /// An operation used a tag with no metadata.
    #[error("operation uses tag `{name}`, which has no metadata in the document `tags`")]
    UndocumentedTag {
        /// The tag used.
        name: String,
    },

    /// A server variable's `enum` was empty.
    #[error("server variable `{name}` declares an empty `enum`")]
    EmptyServerVariableEnum {
        /// The variable name.
        name: String,
    },

    /// A server variable's default was not among its permitted values.
    #[error("server variable `{name}` has a default that is not in its `enum`")]
    ServerVariableDefaultNotInEnum {
        /// The variable name.
        name: String,
    },

    /// An extension field name was malformed or reserved.
    #[error(
        "`{name}` is not a usable extension name: expected an `x-` prefix, not `x-oai-`/`x-oas-`"
    )]
    InvalidExtensionName {
        /// The offending field name.
        name: String,
    },

    /// A schema was deliberately left unconstrained.
    #[error(
        "this payload is described by the permissive schema, so the description does not \
         constrain it"
    )]
    UncheckedSchema,

    /// The description does not fully describe the service.
    #[error("this description omits part of the service and is not authoritative")]
    NotAuthoritative,

    /// The document declared nothing at all.
    #[error("a document must declare at least one of `paths`, `components` or `webhooks`")]
    EmptyDocument,

    /// The document uses constructs that only OpenAPI 3.2 can express.
    #[error("cannot emit as OpenAPI 3.1: {} 3.2-only construct(s) in use: {}", blockers.len(), blockers.join(", "))]
    RequiresV3_2 {
        /// Locations of the constructs standing in the way.
        blockers: Vec<String>,
    },
}

/// Checks a document against the rules of a specification version.
#[derive(Clone, Copy, Debug)]
pub struct Validator {
    version: SpecVersion,
}

impl Validator {
    /// Creates a validator for `version`.
    #[must_use]
    pub fn new(version: SpecVersion) -> Self {
        Self { version }
    }

    /// Collects every violation in `document`, most structural first.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn validate(&self, document: &Document) -> Vec<Violation> {
        let mut violations = Vec::new();

        // OpenAPI 3.2 requires a document to declare something; 3.1 does not.
        if self.version.supports_3_2()
            && document.paths.is_empty()
            && document.webhooks.is_empty()
            && document.components.is_empty()
        {
            violations.push(Violation::error("#", SpecError::EmptyDocument));
        }

        if document
            .info
            .license
            .as_ref()
            .is_some_and(|license| license.identifier.is_some() && license.url.is_some())
        {
            violations.push(Violation::error(
                "#/info/license",
                SpecError::LicenseExclusivity,
            ));
        }

        self.check_servers(document, &mut violations);
        self.check_tags(document, &mut violations);
        self.check_component_names(document, &mut violations);
        self.check_paths(document, &mut violations);
        check_extensions("#", &document.extensions, &mut violations);

        violations
    }

    fn check_servers(self, document: &Document, violations: &mut Vec<Violation>) {
        for (index, server) in document.servers.iter().enumerate() {
            for (name, variable) in &server.variables {
                let location = format!("#/servers/{index}/variables/{name}");
                if let Some(values) = &variable.enumeration {
                    if values.is_empty() {
                        violations.push(Violation::error(
                            &location,
                            SpecError::EmptyServerVariableEnum { name: name.clone() },
                        ));
                    } else if !values.contains(&variable.default_value) {
                        violations.push(Violation::error(
                            &location,
                            SpecError::ServerVariableDefaultNotInEnum { name: name.clone() },
                        ));
                    }
                }
            }
        }
    }

    fn check_tags(self, document: &Document, violations: &mut Vec<Violation>) {
        let mut seen: HashSet<&str> = HashSet::new();
        for (index, tag) in document.tags.iter().enumerate() {
            if !seen.insert(tag.name.as_str()) {
                violations.push(Violation::error(
                    format!("#/tags/{index}"),
                    SpecError::DuplicateTag {
                        name: tag.name.clone(),
                    },
                ));
            }
        }

        #[cfg(feature = "openapi32")]
        self.check_tag_hierarchy(document, &seen, violations);

        #[cfg(not(feature = "openapi32"))]
        let _ = &seen;
    }

    #[cfg(feature = "openapi32")]
    fn check_tag_hierarchy(
        self,
        document: &Document,
        declared: &HashSet<&str>,
        violations: &mut Vec<Violation>,
    ) {
        let parents: HashMap<&str, &str> = document
            .tags
            .iter()
            .filter_map(|tag| {
                tag.parent
                    .as_deref()
                    .map(|parent| (tag.name.as_str(), parent))
            })
            .collect();

        for (index, tag) in document.tags.iter().enumerate() {
            let Some(parent) = tag.parent.as_deref() else {
                continue;
            };
            let location = format!("#/tags/{index}");

            if !declared.contains(parent) {
                violations.push(Violation::error(
                    &location,
                    SpecError::UnknownTagParent {
                        name: tag.name.clone(),
                        parent: parent.to_owned(),
                    },
                ));
                continue;
            }

            // Walk upward, bounded by the number of tags: a chain longer than
            // that has necessarily revisited a node.
            let mut current = parent;
            let mut steps = 0;
            while let Some(next) = parents.get(current) {
                if *next == tag.name.as_str() || steps > document.tags.len() {
                    violations.push(Violation::error(
                        &location,
                        SpecError::TagParentCycle {
                            name: tag.name.clone(),
                        },
                    ));
                    break;
                }
                current = next;
                steps += 1;
            }
        }
    }

    fn check_component_names(self, document: &Document, violations: &mut Vec<Violation>) {
        let components = &document.components;
        let groups: [(&str, Vec<&String>); 5] = [
            ("schemas", components.schemas.keys().collect()),
            ("responses", components.responses.keys().collect()),
            ("parameters", components.parameters.keys().collect()),
            ("requestBodies", components.request_bodies.keys().collect()),
            (
                "securitySchemes",
                components.security_schemes.keys().collect(),
            ),
        ];

        for (group, names) in groups {
            for name in names {
                if !ComponentName::is_valid(name) {
                    violations.push(Violation::error(
                        format!("#/components/{group}/{name}"),
                        SpecError::InvalidComponentName { name: name.clone() },
                    ));
                }
            }
        }
    }

    fn check_paths(self, document: &Document, violations: &mut Vec<Violation>) {
        let declared_schemes: HashSet<&str> = document
            .components
            .security_schemes
            .keys()
            .map(String::as_str)
            .collect();
        let declared_tags: HashSet<&str> =
            document.tags.iter().map(|tag| tag.name.as_str()).collect();

        let mut operation_ids: HashMap<&str, String> = HashMap::new();
        let mut normalized_paths: HashMap<String, &String> = HashMap::new();

        for (raw, item) in &document.paths.0 {
            let location = format!("#/paths/{raw}");

            let Ok(template) = PathTemplate::parse(raw.clone()) else {
                // An unparseable key cannot be checked further, and the parse
                // error itself is reported by whoever constructed it.
                continue;
            };

            if let Some(existing) = normalized_paths.insert(template.normalized(), raw) {
                if existing != raw {
                    violations.push(Violation::error(
                        &location,
                        SpecError::DuplicatePathTemplate {
                            template: raw.clone(),
                            existing: existing.clone(),
                        },
                    ));
                }
            }

            check_parameter_list(&location, &item.parameters, violations);

            for (method, operation) in item.operations() {
                let op_location = format!("{location}/{}", method.as_wire_str().to_lowercase());
                self.check_operation(
                    &op_location,
                    &template,
                    item,
                    operation,
                    &declared_schemes,
                    &declared_tags,
                    &mut operation_ids,
                    violations,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn check_operation<'doc>(
        self,
        location: &str,
        template: &PathTemplate,
        item: &PathItem,
        operation: &'doc Operation,
        declared_schemes: &HashSet<&str>,
        declared_tags: &HashSet<&str>,
        operation_ids: &mut HashMap<&'doc str, String>,
        violations: &mut Vec<Violation>,
    ) {
        if let Some(id) = operation.operation_id.as_deref() {
            if let Some(first) = operation_ids.insert(id, location.to_owned()) {
                violations.push(Violation::error(
                    location,
                    SpecError::DuplicateOperationId {
                        operation_id: id.to_owned(),
                        first,
                    },
                ));
            }
        }

        if operation.responses.is_empty() {
            violations.push(Violation::error(location, SpecError::NoResponses));
        }

        for tag in &operation.tags {
            if !declared_tags.contains(tag.as_str()) {
                violations.push(Violation::warning(
                    location,
                    SpecError::UndocumentedTag { name: tag.clone() },
                ));
            }
        }

        for requirement in operation.security.iter().flatten() {
            for name in requirement.0.keys() {
                if !declared_schemes.contains(name.as_str()) {
                    violations.push(Violation::error(
                        location,
                        SpecError::UnknownSecurityScheme { name: name.clone() },
                    ));
                }
            }
        }

        check_parameter_list(location, &operation.parameters, violations);
        check_path_correspondence(location, template, item, operation, violations);
        check_operation_content(location, operation, violations);

        check_extensions(location, &operation.extensions, violations);
    }
}

/// Checks that path template variables and `in: path` parameters agree.
///
/// Parameters hoisted onto the enclosing Path Item count towards the
/// correspondence, so a shared parameter does not have to be repeated on every
/// operation.
fn check_path_correspondence(
    location: &str,
    template: &PathTemplate,
    item: &PathItem,
    operation: &Operation,
    violations: &mut Vec<Violation>,
) {
    let declared: HashSet<&str> = item
        .parameters
        .iter()
        .chain(operation.parameters.iter())
        .filter_map(RefOr::as_item)
        .filter(|parameter| parameter.location == ParameterIn::Path)
        .map(|parameter| parameter.name.as_str())
        .collect();

    for variable in template.variables() {
        if !declared.contains(variable.as_str()) {
            violations.push(Violation::error(
                location,
                SpecError::UndeclaredPathVariable {
                    name: variable.clone(),
                },
            ));
        }
    }
    for name in &declared {
        if !template.variables().iter().any(|v| v == name) {
            violations.push(Violation::error(
                location,
                SpecError::UnusedPathParameter {
                    name: (*name).to_owned(),
                },
            ));
        }
    }
}

/// Checks the request body and every response of one operation.
fn check_operation_content(location: &str, operation: &Operation, violations: &mut Vec<Violation>) {
    if let Some(RefOr::Item(body)) = &operation.request_body {
        for (media_type, content) in &body.content {
            check_media_type(
                &format!("{location}/requestBody/{media_type}"),
                content,
                violations,
            );
        }
    }

    for (status, response) in &operation.responses.responses {
        let Some(response) = response.as_item() else {
            continue;
        };
        let response_location = format!("{location}/responses/{status}");
        for (media_type, content) in &response.content {
            check_media_type(
                &format!("{response_location}/content/{media_type}"),
                content,
                violations,
            );
        }
        for (name, header) in &response.headers {
            if let Some(header) = header.as_item() {
                check_header(
                    &format!("{response_location}/headers/{name}"),
                    name,
                    header,
                    violations,
                );
            }
        }
        for (name, link) in &response.links {
            if let Some(link) = link.as_item() {
                let set = usize::from(link.operation_ref.is_some())
                    + usize::from(link.operation_id.is_some());
                if set != 1 {
                    violations.push(Violation::error(
                        format!("{response_location}/links/{name}"),
                        SpecError::LinkTargetExclusivity,
                    ));
                }
            }
        }
    }
}

fn check_parameter_list(
    location: &str,
    parameters: &[RefOr<Parameter>],
    violations: &mut Vec<Violation>,
) {
    let mut seen: HashSet<(String, ParameterIn)> = HashSet::new();

    for parameter in parameters.iter().filter_map(RefOr::as_item) {
        let key = (parameter.name.clone(), parameter.location);
        if !seen.insert(key) {
            violations.push(Violation::error(
                location,
                SpecError::DuplicateParameter {
                    name: parameter.name.clone(),
                    location: format!("{:?}", parameter.location).to_lowercase(),
                },
            ));
        }

        if parameter.location == ParameterIn::Header && is_ignored_header_parameter(&parameter.name)
        {
            violations.push(Violation::error(
                location,
                SpecError::IgnoredHeaderParameter {
                    name: parameter.name.clone(),
                },
            ));
        }

        if parameter.location == ParameterIn::Path && parameter.required != Some(true) {
            violations.push(Violation::error(
                location,
                SpecError::PathParameterNotRequired {
                    name: parameter.name.clone(),
                },
            ));
        }

        let has_schema = parameter.schema.is_some();
        let has_content = !parameter.content.is_empty();
        if has_schema == has_content {
            violations.push(Violation::error(
                location,
                SpecError::SchemaContentExclusivity {
                    name: parameter.name.clone(),
                },
            ));
        } else if has_content && parameter.content.len() != 1 {
            violations.push(Violation::error(
                location,
                SpecError::ContentNotSingular {
                    name: parameter.name.clone(),
                    found: parameter.content.len(),
                },
            ));
        }

        if let Some(style) = parameter.style {
            if !style.is_valid_for(parameter.location) {
                violations.push(Violation::error(
                    location,
                    SpecError::IllegalStyle {
                        style: format!("{style:?}").to_lowercase(),
                        location: format!("{:?}", parameter.location).to_lowercase(),
                    },
                ));
            }
        }

        if parameter.example.is_some() && !parameter.examples.is_empty() {
            violations.push(Violation::error(location, SpecError::ExampleExclusivity));
        }

        check_extensions(location, &parameter.extensions, violations);
    }
}

fn check_header(location: &str, name: &str, header: &Header, violations: &mut Vec<Violation>) {
    let has_schema = header.schema.is_some();
    let has_content = !header.content.is_empty();
    if has_schema == has_content {
        violations.push(Violation::error(
            location,
            SpecError::SchemaContentExclusivity {
                name: name.to_owned(),
            },
        ));
    } else if has_content && header.content.len() != 1 {
        violations.push(Violation::error(
            location,
            SpecError::ContentNotSingular {
                name: name.to_owned(),
                found: header.content.len(),
            },
        ));
    }

    if header.example.is_some() && !header.examples.is_empty() {
        violations.push(Violation::error(location, SpecError::ExampleExclusivity));
    }
}

fn check_media_type(location: &str, content: &MediaType, violations: &mut Vec<Violation>) {
    if content.example.is_some() && !content.examples.is_empty() {
        violations.push(Violation::error(location, SpecError::ExampleExclusivity));
    }

    if let Some(schema) = &content.schema {
        if is_unchecked(schema) {
            violations.push(Violation::warning(location, SpecError::UncheckedSchema));
        }
    }
}

fn is_unchecked(schema: &crate::schema::Schema) -> bool {
    match schema {
        crate::schema::Schema::Bool(true) => true,
        crate::schema::Schema::Object(object) => object
            .unknown_keywords
            .contains_key(UNCHECKED_SCHEMA_ANNOTATION),
        crate::schema::Schema::Bool(false) => false,
    }
}

fn check_extensions(location: &str, extensions: &Extensions, violations: &mut Vec<Violation>) {
    for name in extensions.0.keys() {
        if !Extensions::is_valid_name(name) {
            violations.push(Violation::warning(
                location,
                SpecError::InvalidExtensionName { name: name.clone() },
            ));
        }
    }
}

/// Lists the OpenAPI 3.2-only constructs a document uses.
///
/// Each entry is a location, suitable for telling the caller what stands in the
/// way of emitting the document as 3.1. Always empty in a build without the
/// `openapi32` feature, since the constructs cannot be represented at all.
#[must_use]
pub fn three_two_only_constructs(document: &Document) -> Vec<String> {
    #[cfg(not(feature = "openapi32"))]
    {
        let _ = document;
        Vec::new()
    }

    #[cfg(feature = "openapi32")]
    {
        let mut blockers = Vec::new();

        if document.self_uri.is_some() {
            blockers.push("#/$self".to_owned());
        }
        for (index, server) in document.servers.iter().enumerate() {
            if server.name.is_some() {
                blockers.push(format!("#/servers/{index}/name"));
            }
        }
        for (index, tag) in document.tags.iter().enumerate() {
            for (field, present) in [
                ("summary", tag.summary.is_some()),
                ("parent", tag.parent.is_some()),
                ("kind", tag.kind.is_some()),
            ] {
                if present {
                    blockers.push(format!("#/tags/{index}/{field}"));
                }
            }
        }
        if !document.components.media_types.is_empty() {
            blockers.push("#/components/mediaTypes".to_owned());
        }

        for (raw, item) in &document.paths.0 {
            let location = format!("#/paths/{raw}");
            if item.query.is_some() {
                blockers.push(format!("{location}/query"));
            }
            if !item.additional_operations.is_empty() {
                blockers.push(format!("{location}/additionalOperations"));
            }
            for (method, operation) in item.operations() {
                let op = format!("{location}/{}", method.as_wire_str().to_lowercase());
                collect_operation_blockers(&op, operation, &mut blockers);
            }
        }

        blockers
    }
}

#[cfg(feature = "openapi32")]
fn collect_operation_blockers(location: &str, operation: &Operation, blockers: &mut Vec<String>) {
    for parameter in operation.parameters.iter().filter_map(RefOr::as_item) {
        if parameter.location == ParameterIn::Querystring {
            blockers.push(format!("{location}/parameters/{}", parameter.name));
        }
        if parameter.style == Some(crate::parameter::Style::Cookie) {
            blockers.push(format!("{location}/parameters/{}/style", parameter.name));
        }
    }

    if let Some(RefOr::Item(body)) = &operation.request_body {
        for (media_type, content) in &body.content {
            collect_media_type_blockers(
                &format!("{location}/requestBody/content/{media_type}"),
                content,
                blockers,
            );
        }
    }

    for (status, response) in &operation.responses.responses {
        let Some(response) = response.as_item() else {
            continue;
        };
        if response.summary.is_some() {
            blockers.push(format!("{location}/responses/{status}/summary"));
        }
        for (media_type, content) in &response.content {
            collect_media_type_blockers(
                &format!("{location}/responses/{status}/content/{media_type}"),
                content,
                blockers,
            );
        }
    }
}

#[cfg(feature = "openapi32")]
fn collect_media_type_blockers(location: &str, content: &MediaType, blockers: &mut Vec<String>) {
    for (field, present) in [
        ("itemSchema", content.item_schema.is_some()),
        ("prefixEncoding", content.prefix_encoding.is_some()),
        ("itemEncoding", content.item_encoding.is_some()),
    ] {
        if present {
            blockers.push(format!("{location}/{field}"));
        }
    }
}

impl Document {
    /// Validates this document against the rules of `version`.
    ///
    /// # Errors
    ///
    /// Returns every [`Severity::Error`] violation found. Warnings are
    /// discarded; use [`Validator::validate`] to see them.
    pub fn validate(&self, version: SpecVersion) -> Result<(), Vec<Violation>> {
        let errors: Vec<Violation> = Validator::new(version)
            .validate(self)
            .into_iter()
            .filter(|violation| violation.severity == Severity::Error)
            .collect();

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

// `Map` is used by the 3.2-gated paths only; naming it here keeps the import
// list stable across feature combinations.
#[allow(unused)]
type UnusedMapAlias = Map<()>;

#[cfg(test)]
mod tests {
    use super::{SpecError, Validator, Violation};
    use crate::{
        Document, Info, Response, Responses, SpecVersion,
        parameter::{Parameter, Style},
        paths::{Method, Operation, PathItem, PathTemplate},
        schema::{Schema, SchemaType},
        security::SecurityRequirement,
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
}
