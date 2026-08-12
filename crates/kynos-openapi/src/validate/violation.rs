//! What a validator reports: how bad it is, where it is, and what is wrong.

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
    pub(in crate::validate) fn error(location: impl Into<String>, error: SpecError) -> Self {
        Self {
            location: location.into(),
            severity: Severity::Error,
            error,
        }
    }

    pub(in crate::validate) fn warning(location: impl Into<String>, error: SpecError) -> Self {
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

/// A violation is a line in a report rather than a link in a chain, so it
/// carries no cause.
///
/// A validation run yields a list, and every consumer prints that list —
/// `Router::validate` hands one back, and `Error::Invalid` renders one. `Display`
/// is self-contained for that reason, so offering the [`SpecError`] it already
/// names as a `source()` would make any reporter print the same sentence twice.
///
/// The implementation is still worth having: it is what lets a single violation
/// be boxed, returned through `?`, or downcast back out of a `dyn Error`.
impl std::error::Error for Violation {}

/// Escapes one map key for use as a JSON Pointer token, per RFC 6901.
///
/// Every `paths` key contains a `/`, so a location that embeds one unescaped
/// reads as several tokens and resolves against nothing. Shared so that the
/// three places that build locations cannot disagree.
pub(crate) fn pointer_token(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
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

    /// A `paths` key is not a legal path template.
    ///
    /// Reachable only for a description read from somewhere else: a template
    /// Kynos constructs is checked when it is parsed.
    ///
    /// `reason` holds the parse failure itself rather than its text, so a caller
    /// can match on which rule the key broke. It is interpolated rather than
    /// declared as a `#[source]`: a violation is rendered in a list, so its
    /// message has to be self-contained, and a cause could then only repeat it.
    #[error("`{template}` is not a legal path template: {reason}")]
    InvalidPathTemplate {
        /// The offending key.
        template: String,
        /// Why it is not one.
        reason: crate::model::paths::template::InvalidPathTemplate,
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

    /// An operation is emitted but is covered by an `unchecked` waiver.
    ///
    /// It is still described; what it does is no longer verified.
    #[error(
        "this operation is covered by an `unchecked` waiver ({}), so its description is not \
         verified",
        reasons.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
    )]
    OpaqueOperation {
        /// Why the waiver was needed.
        reasons: Vec<crate::annotation::OpaqueReason>,
    },

    /// A route is served but has no path template that could express it.
    #[error(
        "`{pattern}` is served but no path template can express it, so it has no `paths` entry"
    )]
    OpaqueRoute {
        /// The router's matching pattern, verbatim.
        pattern: String,
    },

    /// The description is opaque somewhere but does not say so at the root.
    ///
    /// The document-level stamp is the one-glance signal a consumer reads
    /// before deciding whether to trust anything else, so a description that
    /// omits it while carrying an opaque operation or route is worse than one
    /// that is honestly incomplete.
    #[error(
        "this description contains opaque operations or routes but is not marked \
         non-authoritative"
    )]
    AuthorityNotStamped,

    /// A Kynos annotation was present but not in the shape Kynos emits.
    ///
    /// `detail` is text where
    /// [`InvalidPathTemplate`](Self::InvalidPathTemplate)'s `reason` is a value,
    /// and the asymmetry is forced rather than chosen: this cause is a
    /// `serde_json::Error`, which is neither `Clone` nor `PartialEq`, so keeping
    /// it would cost the derives every other value in the validation model has.
    #[error("`{name}` is present but is not in the form Kynos emits: {detail}")]
    MalformedAnnotation {
        /// The offending field name.
        name: String,
        /// What went wrong reading it.
        detail: String,
    },

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
