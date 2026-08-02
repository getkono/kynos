//! The Paths, Path Item and Operation Objects, and path templating.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{
    Map, body::RequestBody, callback::Callback, extensions::Extensions,
    external_docs::ExternalDocumentation, parameter::Parameter, reference::RefOr,
    response::Responses, security::SecurityRequirement, server::Server,
};

/// An HTTP method that has a dedicated Path Item field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    /// `GET`.
    Get,
    /// `PUT`.
    Put,
    /// `POST`.
    Post,
    /// `DELETE`.
    Delete,
    /// `OPTIONS`.
    Options,
    /// `HEAD`.
    Head,
    /// `PATCH`.
    Patch,
    /// `TRACE`.
    Trace,
    /// `QUERY`, as defined by the HTTP QUERY method draft.
    ///
    /// Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    Query,
}

impl Method {
    /// Every method with a dedicated Path Item field.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::Get,
            Self::Put,
            Self::Post,
            Self::Delete,
            Self::Options,
            Self::Head,
            Self::Patch,
            Self::Trace,
            #[cfg(feature = "openapi32")]
            Self::Query,
        ]
    }

    /// The method name as it appears on the wire.
    #[must_use]
    pub fn as_wire_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Put => "PUT",
            Self::Post => "POST",
            Self::Delete => "DELETE",
            Self::Options => "OPTIONS",
            Self::Head => "HEAD",
            Self::Patch => "PATCH",
            Self::Trace => "TRACE",
            #[cfg(feature = "openapi32")]
            Self::Query => "QUERY",
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire_str())
    }
}

/// The error returned when a path template is malformed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidPathTemplate {
    /// The template did not begin with `/`.
    #[error("path template `{0}` must begin with `/`")]
    MissingLeadingSlash(String),

    /// A `{` was opened but never closed, or a `}` appeared unopened.
    #[error("path template `{0}` has unbalanced braces")]
    UnbalancedBraces(String),

    /// A `{}` expression contained no name.
    #[error("path template `{0}` contains an empty `{{}}` expression")]
    EmptyExpression(String),

    /// The same variable name appeared more than once.
    ///
    /// A template expression must not be repeated within one path.
    #[error("path template `{template}` repeats the variable `{name}`")]
    DuplicateVariable {
        /// The offending template.
        template: String,
        /// The variable that appeared more than once.
        name: String,
    },

    /// The template contained a query string or fragment.
    #[error("path template `{0}` must not contain a query string or fragment")]
    NotAPath(String),
}

/// A parsed path template such as `/users/{id}/posts/{postId}`.
///
/// Two templates that differ only in variable name are *the same path* as far
/// as OpenAPI is concerned, so declaring both is invalid.
/// [`normalized`](PathTemplate::normalized) exists to make that comparison
/// cheap.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PathTemplate {
    raw: String,
    variables: Vec<String>,
}

impl PathTemplate {
    /// Parses a path template.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidPathTemplate`] when the template does not start with
    /// `/`, has unbalanced or empty braces, repeats a variable, or carries a
    /// query string or fragment.
    pub fn parse(raw: impl Into<String>) -> Result<Self, InvalidPathTemplate> {
        let raw = raw.into();

        if !raw.starts_with('/') {
            return Err(InvalidPathTemplate::MissingLeadingSlash(raw));
        }
        if raw.contains('?') || raw.contains('#') {
            return Err(InvalidPathTemplate::NotAPath(raw));
        }

        let mut variables = Vec::new();
        let mut rest = raw.as_str();
        while let Some(open) = rest.find('{') {
            let after_open = &rest[open + 1..];
            let Some(close) = after_open.find('}') else {
                return Err(InvalidPathTemplate::UnbalancedBraces(raw));
            };
            let name = &after_open[..close];
            if name.is_empty() {
                return Err(InvalidPathTemplate::EmptyExpression(raw));
            }
            if name.contains('{') {
                return Err(InvalidPathTemplate::UnbalancedBraces(raw));
            }
            if variables.iter().any(|existing| existing == name) {
                return Err(InvalidPathTemplate::DuplicateVariable {
                    name: name.to_owned(),
                    template: raw,
                });
            }
            variables.push(name.to_owned());
            rest = &after_open[close + 1..];
        }
        if rest.contains('}') {
            return Err(InvalidPathTemplate::UnbalancedBraces(raw));
        }

        Ok(Self { raw, variables })
    }

    /// The template exactly as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// The variable names, in the order they appear.
    #[must_use]
    pub fn variables(&self) -> &[String] {
        &self.variables
    }

    /// The template with every variable name replaced by `{}`.
    ///
    /// Two templates are the same path if and only if their normalized forms
    /// are equal.
    #[must_use]
    pub fn normalized(&self) -> String {
        let mut out = String::with_capacity(self.raw.len());
        let mut rest = self.raw.as_str();
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            out.push_str("{}");
            let after_open = &rest[open + 1..];
            let close = after_open.find('}').expect("parse validated the braces");
            rest = &after_open[close + 1..];
        }
        out.push_str(rest);
        out
    }

    /// Concatenates a prefix onto this template, as nesting does.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidPathTemplate`] when the result is not a valid template,
    /// which is how a prefix that repeats one of this template's variables is
    /// caught.
    pub fn with_prefix(&self, prefix: &str) -> Result<Self, InvalidPathTemplate> {
        let prefix = prefix.trim_end_matches('/');
        if prefix.is_empty() {
            return Ok(self.clone());
        }
        Self::parse(format!("{prefix}{}", self.raw))
    }
}

impl fmt::Display for PathTemplate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

impl FromStr for PathTemplate {
    type Err = InvalidPathTemplate;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl TryFrom<String> for PathTemplate {
    type Error = InvalidPathTemplate;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<PathTemplate> for String {
    fn from(template: PathTemplate) -> Self {
        template.raw
    }
}

/// The available paths and the operations on each.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Paths(pub Map<PathItem>);

impl Paths {
    /// Creates an empty path map.
    #[must_use]
    pub fn new() -> Self {
        Self(Map::new())
    }

    /// Inserts a path item, replacing any existing entry for that template.
    pub fn insert(&mut self, template: &PathTemplate, item: PathItem) -> Option<PathItem> {
        self.0.insert(template.as_str().to_owned(), item)
    }

    /// Looks up the path item for a template.
    #[must_use]
    pub fn get(&self, template: &PathTemplate) -> Option<&PathItem> {
        self.0.get(template.as_str())
    }

    /// Returns `true` when no path is declared.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The operations available on a single path.
///
/// The per-method slots are boxed. An [`Operation`] is over a kilobyte, and
/// inlining nine of them made this type 8.7 KB — a cost every `Paths` entry
/// paid on insert and on rehash. Use [`operation`](PathItem::operation) and
/// [`set_operation`](PathItem::set_operation) rather than touching the fields,
/// and the indirection stays invisible.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PathItem {
    /// A reference to an external Path Item.
    ///
    /// The specification leaves the behaviour of fields adjacent to this
    /// undefined, so Kynos never emits one.
    #[serde(rename = "$ref", default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,

    /// A summary applying to every operation on this path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// A description applying to every operation on this path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The `GET` operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub get: Option<Box<Operation>>,

    /// The `PUT` operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub put: Option<Box<Operation>>,

    /// The `POST` operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post: Option<Box<Operation>>,

    /// The `DELETE` operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete: Option<Box<Operation>>,

    /// The `OPTIONS` operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Box<Operation>>,

    /// The `HEAD` operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<Box<Operation>>,

    /// The `PATCH` operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<Box<Operation>>,

    /// The `TRACE` operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<Box<Operation>>,

    /// The `QUERY` operation.
    ///
    /// Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<Box<Operation>>,

    /// Operations for methods with no dedicated field.
    ///
    /// Introduced in OpenAPI 3.2. Keys are HTTP methods with the exact
    /// capitalization sent on the wire, and must not duplicate a method that
    /// has its own field.
    #[cfg(feature = "openapi32")]
    #[serde(
        rename = "additionalOperations",
        default,
        skip_serializing_if = "Map::is_empty"
    )]
    pub additional_operations: Map<Box<Operation>>,

    /// Servers serving this path, overriding the document-level list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<Server>,

    /// Parameters applying to every operation on this path.
    ///
    /// Hoisting shared parameters here rather than repeating them on each
    /// operation is what keeps a large description readable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<RefOr<Parameter>>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl PathItem {
    /// Creates a path item with no operations.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the operation for a method, if declared.
    #[must_use]
    pub fn operation(&self, method: Method) -> Option<&Operation> {
        match method {
            Method::Get => self.get.as_deref(),
            Method::Put => self.put.as_deref(),
            Method::Post => self.post.as_deref(),
            Method::Delete => self.delete.as_deref(),
            Method::Options => self.options.as_deref(),
            Method::Head => self.head.as_deref(),
            Method::Patch => self.patch.as_deref(),
            Method::Trace => self.trace.as_deref(),
            #[cfg(feature = "openapi32")]
            Method::Query => self.query.as_deref(),
        }
    }

    /// Sets the operation for a method, returning any operation it replaced.
    pub fn set_operation(&mut self, method: Method, operation: Operation) -> Option<Operation> {
        let slot = match method {
            Method::Get => &mut self.get,
            Method::Put => &mut self.put,
            Method::Post => &mut self.post,
            Method::Delete => &mut self.delete,
            Method::Options => &mut self.options,
            Method::Head => &mut self.head,
            Method::Patch => &mut self.patch,
            Method::Trace => &mut self.trace,
            #[cfg(feature = "openapi32")]
            Method::Query => &mut self.query,
        };
        slot.replace(Box::new(operation)).map(|boxed| *boxed)
    }

    /// Iterates over the declared operations and their methods.
    pub fn operations(&self) -> impl Iterator<Item = (Method, &Operation)> {
        Method::all()
            .iter()
            .filter_map(move |&method| self.operation(method).map(|op| (method, op)))
    }

    /// Sets the operation for a method, in builder style.
    #[must_use]
    pub fn with_operation(mut self, method: Method, operation: Operation) -> Self {
        self.set_operation(method, operation);
        self
    }
}

/// A single API operation on a path.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    /// Tags for grouping this operation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    /// A short summary of what the operation does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// A verbose explanation. [CommonMark] syntax may be used.
    ///
    /// [CommonMark]: https://spec.commonmark.org/
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Additional external documentation.
    #[serde(
        rename = "externalDocs",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub external_docs: Option<ExternalDocumentation>,

    /// A case-sensitive identifier, unique across the whole description.
    ///
    /// Optional per the specification, but Kynos always emits one: it is what
    /// client generators name their methods after, and what a
    /// [`Link`](crate::Link) refers to.
    #[serde(
        rename = "operationId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub operation_id: Option<String>,

    /// Parameters applying to this operation.
    ///
    /// An entry here with the same name and location as one on the enclosing
    /// [`PathItem`] overrides it, but cannot remove it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<RefOr<Parameter>>,

    /// The request body.
    #[serde(
        rename = "requestBody",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub request_body: Option<RefOr<RequestBody>>,

    /// The responses this operation may return.
    #[serde(default, skip_serializing_if = "Responses::is_empty")]
    pub responses: Responses,

    /// Out-of-band requests made as part of this operation.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub callbacks: Map<RefOr<Callback>>,

    /// Whether this operation is deprecated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<bool>,

    /// The security requirements, overriding the document-level list.
    ///
    /// An empty vector is *not* the same as absent: it removes the
    /// document-level requirement, making the operation anonymous.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security: Option<Vec<SecurityRequirement>>,

    /// Servers serving this operation, overriding wider declarations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<Server>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl Operation {
    /// Creates an operation identified by `operation_id`.
    pub fn new(operation_id: impl Into<String>) -> Self {
        Self {
            operation_id: Some(operation_id.into()),
            ..Self::default()
        }
    }

    /// Sets the summary.
    #[must_use]
    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// Sets the description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds a tag.
    #[must_use]
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Adds a parameter.
    #[must_use]
    pub fn with_parameter(mut self, parameter: Parameter) -> Self {
        self.parameters.push(RefOr::Item(parameter));
        self
    }

    /// Sets the request body.
    #[must_use]
    pub fn with_request_body(mut self, body: RequestBody) -> Self {
        self.request_body = Some(RefOr::Item(body));
        self
    }

    /// Sets the responses.
    #[must_use]
    pub fn with_responses(mut self, responses: Responses) -> Self {
        self.responses = responses;
        self
    }

    /// Adds a security requirement.
    #[must_use]
    pub fn with_security(mut self, requirement: SecurityRequirement) -> Self {
        self.security.get_or_insert_with(Vec::new).push(requirement);
        self
    }

    /// Marks the operation deprecated.
    #[must_use]
    pub fn deprecated(mut self) -> Self {
        self.deprecated = Some(true);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{InvalidPathTemplate, Method, Operation, PathItem, PathTemplate};

    #[test]
    fn a_template_exposes_its_variables_in_order() {
        let template = PathTemplate::parse("/users/{id}/posts/{postId}").expect("valid");
        assert_eq!(template.variables(), ["id", "postId"]);
    }

    #[test]
    fn a_template_without_variables_is_valid() {
        let template = PathTemplate::parse("/health").expect("valid");
        assert!(template.variables().is_empty());
        assert_eq!(template.normalized(), "/health");
    }

    #[test]
    fn templates_must_begin_with_a_slash() {
        assert!(matches!(
            PathTemplate::parse("users/{id}"),
            Err(InvalidPathTemplate::MissingLeadingSlash(_))
        ));
    }

    #[test]
    fn braces_must_balance() {
        assert!(matches!(
            PathTemplate::parse("/users/{id"),
            Err(InvalidPathTemplate::UnbalancedBraces(_))
        ));
        assert!(matches!(
            PathTemplate::parse("/users/id}"),
            Err(InvalidPathTemplate::UnbalancedBraces(_))
        ));
        assert!(matches!(
            PathTemplate::parse("/users/{{id}"),
            Err(InvalidPathTemplate::UnbalancedBraces(_))
        ));
    }

    #[test]
    fn an_empty_expression_is_rejected() {
        assert!(matches!(
            PathTemplate::parse("/users/{}"),
            Err(InvalidPathTemplate::EmptyExpression(_))
        ));
    }

    #[test]
    fn a_variable_may_not_repeat_within_one_template() {
        assert!(matches!(
            PathTemplate::parse("/a/{id}/b/{id}"),
            Err(InvalidPathTemplate::DuplicateVariable { .. })
        ));
    }

    #[test]
    fn query_strings_and_fragments_are_not_paths() {
        assert!(matches!(
            PathTemplate::parse("/users?page=1"),
            Err(InvalidPathTemplate::NotAPath(_))
        ));
        assert!(matches!(
            PathTemplate::parse("/users#top"),
            Err(InvalidPathTemplate::NotAPath(_))
        ));
    }

    #[test]
    fn templates_differing_only_in_variable_name_normalize_alike() {
        let left = PathTemplate::parse("/pets/{petId}").expect("valid");
        let right = PathTemplate::parse("/pets/{name}").expect("valid");
        assert_ne!(left, right);
        assert_eq!(left.normalized(), right.normalized());
    }

    #[test]
    fn prefixing_concatenates_and_revalidates() {
        let template = PathTemplate::parse("/users/{id}").expect("valid");
        let prefixed = template.with_prefix("/v1").expect("valid");
        assert_eq!(prefixed.as_str(), "/v1/users/{id}");

        // The prefix reintroduces `id`, which the combined template forbids.
        assert!(matches!(
            template.with_prefix("/tenants/{id}"),
            Err(InvalidPathTemplate::DuplicateVariable { .. })
        ));
    }

    #[test]
    fn a_trailing_slash_on_the_prefix_does_not_double_up() {
        let template = PathTemplate::parse("/users").expect("valid");
        assert_eq!(
            template.with_prefix("/v1/").expect("valid").as_str(),
            "/v1/users"
        );
    }

    #[test]
    fn operations_are_addressed_by_method() {
        let item = PathItem::new().with_operation(Method::Get, Operation::new("listUsers"));
        assert!(item.operation(Method::Get).is_some());
        assert!(item.operation(Method::Post).is_none());
        assert_eq!(item.operations().count(), 1);
    }

    #[test]
    fn methods_render_in_wire_case() {
        assert_eq!(Method::Get.to_string(), "GET");
        assert_eq!(Method::Delete.as_wire_str(), "DELETE");
    }
}
