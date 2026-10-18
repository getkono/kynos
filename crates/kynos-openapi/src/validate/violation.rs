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

/// A violation is a line in a report, so it carries no cause: its `Display`
/// already includes the [`SpecError`].
impl std::error::Error for Violation {}

/// Escapes one map key for use as a JSON Pointer token, per RFC 6901.
///
/// Every `paths` key contains a `/`, which must not split the location.
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

    /// One method on one path was described more than once.
    ///
    /// Reachable only while a description is being assembled: a Path Item
    /// holds one Operation per method, so no document can carry the second.
    #[error(
        "operation `{method} {path}` is described more than once; a path item holds one \
         operation per method"
    )]
    DuplicateOperation {
        /// The method described twice.
        method: crate::model::paths::method::Method,
        /// The `paths` key it was described under.
        path: String,
    },

    /// An `additionalOperations` key names a method with its own Path Item
    /// field.
    ///
    /// 3.2 forbids the entry outright, beside the field or instead of it, so a
    /// reader has one place to look for each method. The match is exact, as a
    /// method token is case-sensitive: `post` is not `POST`, and stays legal.
    #[cfg(feature = "openapi32")]
    #[error(
        "`additionalOperations` must not hold `{method}`: the method has its own Path Item \
         field, which describes it instead"
    )]
    FixedMethodInAdditionalOperations {
        /// The method the key names.
        method: crate::model::paths::method::Method,
    },

    /// A `paths` key is not a legal path template.
    ///
    /// Reachable only for a description read from somewhere else: a template
    /// Kynos constructs is checked when it is parsed.
    ///
    /// `reason` is the parse failure itself, so a caller can match on it; it is
    /// interpolated rather than a `#[source]`, keeping the message self-contained.
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
        /// Where it is carried, as a description spells it.
        location: String,
    },

    /// An `in: querystring` parameter was described by `schema`.
    ///
    /// 3.2 requires `content` there: the value is the whole query string, and
    /// only a media type says how that string encodes it.
    #[cfg(feature = "openapi32")]
    #[error("`in: querystring` parameter `{name}` must be described by `content`, not `schema`")]
    QuerystringWithoutContent {
        /// The offending parameter.
        name: String,
    },

    /// Two `in: querystring` parameters apply to one operation.
    ///
    /// Counted across the operation and its path item, after the operation's
    /// own parameters override the path item's: there is one query string, so
    /// at most one parameter may describe the whole of it.
    #[cfg(feature = "openapi32")]
    #[error(
        "`in: querystring` parameters `{first}` and `{second}` both describe the query string; \
         an operation may declare at most one"
    )]
    DuplicateQuerystring {
        /// The querystring parameter declared first.
        first: String,
        /// The one declared after it.
        second: String,
    },

    /// An `in: query` parameter applies to an operation beside an
    /// `in: querystring` one.
    ///
    /// Counted across the operation and its path item, like
    /// [`DuplicateQuerystring`](Self::DuplicateQuerystring): the querystring
    /// parameter already describes every pair the query parameter names.
    #[cfg(feature = "openapi32")]
    #[error(
        "`in: query` parameter `{query}` must not appear beside `in: querystring` parameter \
         `{querystring}`"
    )]
    QueryBesideQuerystring {
        /// The query parameter.
        query: String,
        /// The querystring parameter it appears beside.
        querystring: String,
    },

    /// An interceptor's short circuit declared statuses its responses do not
    /// describe, or the reverse.
    ///
    /// Only reachable from a hand-written `ShortCircuit`; the one the
    /// `ApiError` derive emits comes from the same declaration as the
    /// responses and cannot disagree with them.
    #[error(
        "`{name}` declares statuses {declared:?} as a short circuit but describes {described:?}"
    )]
    ShortCircuitMismatch {
        /// The short-circuit type.
        name: String,
        /// What its `STATUSES` const claimed.
        declared: Vec<u16>,
        /// What its `Responses` actually described.
        described: Vec<u16>,
    },

    /// A serialization style is not legal at a parameter's location.
    #[error("style `{style}` may not be used with `in: {location}`")]
    IllegalStyle {
        /// The style used, as a description spells it (`deepObject`).
        style: String,
        /// The location it was used at, as a description spells it.
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

    /// A header was declared in a `headers` map despite being ignored.
    ///
    /// The parameter-side counterpart is [`SpecError::IgnoredHeaderParameter`].
    #[error(
        "`{name}` must not be declared here: the media type is stated separately, so \
         the specification says such a definition is ignored"
    )]
    IgnoredHeader {
        /// The offending header name.
        name: String,
    },

    /// An operation declared no responses.
    #[error("an operation must declare at least one response")]
    NoResponses,

    /// A Responses Object key is not one a reader takes back as a status.
    ///
    /// Only `1XX`-`5XX` and three digits beginning `1`-`5` are status keys:
    /// `600` or `42` does not parse, `default` reads back as the fallback
    /// response, and an `x-` key as an extension. Reachable through
    /// [`Responses::with`](crate::model::response::Responses::with), which
    /// takes any `u16`, or the public `responses` map.
    #[error(
        "`{key}` is not a response key a reader takes back: expected a status code from \
         `100` to `599`, or one of `1XX`, `2XX`, `3XX`, `4XX`, `5XX`"
    )]
    InvalidResponseKey {
        /// The offending key.
        key: String,
    },

    /// `encoding` was declared beside `prefixEncoding` or `itemEncoding`.
    ///
    /// 3.2 makes the three mutually exclusive: the first names properties, the
    /// other two name array positions, and an object cannot be both.
    #[cfg(feature = "openapi32")]
    #[error(
        "`encoding` describes named properties and `prefixEncoding`/`itemEncoding` describe array \
         positions; a media type declares one or the other, never both"
    )]
    ConflictingEncoding,

    /// `prefixEncoding` or `itemEncoding` was declared on a media type with
    /// neither `itemSchema` nor an array `schema`.
    ///
    /// 3.2 applies both by array position, so there must be an array for them
    /// to index. Raised only where no array is possible: no `schema`, or one
    /// whose `type` omits `array`, directly, through a `#/components/schemas`
    /// reference, or in an `allOf` member.
    #[cfg(feature = "openapi32")]
    #[error(
        "`prefixEncoding` and `itemEncoding` apply by array position, so the media type needs \
         `itemSchema` or an array `schema`"
    )]
    PositionalEncodingWithoutArray,

    /// An `encoding` key names no property of the media type's schema.
    ///
    /// Raised only where every keyword that could declare the property was
    /// read: `properties`, a `#/components/schemas` reference, and `allOf`. A
    /// schema reaching `anyOf`, `oneOf`, a conditional, `dependentSchemas`, a
    /// boolean schema, or a reference elsewhere could declare it unseen, so
    /// its keys are not judged.
    #[error("encoding key `{property}` names no property of the media type's schema")]
    UnknownEncodingProperty {
        /// The key with no property behind it.
        property: String,
    },

    /// A response carried no description, which 3.1 requires.
    ///
    /// 3.2 makes it optional, so this is raised only when validating against
    /// 3.1.
    #[error("a response must have a description under OpenAPI 3.1")]
    MissingResponseDescription,

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

    /// A Link Object's `operationId` names no operation the description
    /// describes.
    ///
    /// Not judged where a Path Item or Callback is a reference into another
    /// document, which may describe the operation.
    #[error("link `operationId` `{operation_id}` names no operation this description describes")]
    UnknownLinkOperationId {
        /// The identifier the link names.
        operation_id: String,
    },

    /// A Link Object's same-document `operationRef` points at no Operation
    /// Object.
    ///
    /// Only a reference beginning `#` is followed, percent-decoded. One into
    /// another document is not judged, and nor is one passing through a Path
    /// Item or Callback given by reference, whose target is not walked here.
    #[error("link `operationRef` `{operation_ref}` points at no Operation Object in this document")]
    UnresolvedLinkOperationRef {
        /// The reference, as the link spells it.
        operation_ref: String,
    },

    /// An OAuth flow omitted a URL its flow requires.
    ///
    /// Reported once per missing URL, at the flow. `authorizationUrl` is
    /// required by `implicit` and `authorizationCode`; `tokenUrl` by
    /// `password`, `clientCredentials`, `authorizationCode` and, in 3.2,
    /// `deviceAuthorization`, which also requires `deviceAuthorizationUrl`.
    /// Only absence is judged: whether a present value is a URL is not.
    #[error("the `{flow}` OAuth flow must declare `{field}`")]
    MissingOAuthFlowUrl {
        /// The flow, as a description spells it (`authorizationCode`).
        flow: &'static str,
        /// The missing field, as a description spells it (`tokenUrl`).
        field: &'static str,
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

    /// A Callback Object's Path Item was keyed like a specification extension.
    ///
    /// Items and extensions share the callback's one object, told apart by the
    /// `x-` prefix, so a reader takes such an item for an extension and drops
    /// the operations under it.
    #[error(
        "callback expression `{expression}` begins with `x-`, so its Path Item is read back as \
         a specification extension"
    )]
    CallbackKeyedAsExtension {
        /// The offending key.
        expression: String,
    },

    /// A Discriminator Object was written beside no `oneOf`, `anyOf` or
    /// `allOf`.
    ///
    /// Legal only with one of the three (`references/3.1.2.md:3341`,
    /// `references/3.2.0.md:3623`), or on the parent schema of the `allOf`
    /// form: one some `allOf` member's `$ref` names by the pointer this is
    /// reported under, `#/components/schemas/Pet` say. A reference spelled any
    /// other way, through `$id` or an anchor, is not matched. Located at the
    /// `discriminator`.
    #[error(
        "a discriminator must sit beside `oneOf`, `anyOf` or `allOf`, or on a schema an `allOf` \
         member references"
    )]
    DiscriminatorWithoutComposition,

    /// A discriminating property was not required, which 3.1 requires.
    ///
    /// `references/3.1.2.md:2834`. Raised only when validating against 3.1, and
    /// only where the property is provably optional: read are `required`,
    /// every `allOf` member, every `oneOf` or `anyOf` branch together, both
    /// arms of a conditional, and the `#/components/schemas` entry a `$ref`
    /// names. A schema carrying a `$dynamicRef`, a reference elsewhere, `not`,
    /// `dependentSchemas` or `dependentRequired` could require it unseen, so
    /// counts as requiring it. Located at the `discriminator`.
    #[error("discriminating property `{property}` must be required under OpenAPI 3.1")]
    DiscriminatorPropertyNotRequired {
        /// The `propertyName`.
        property: String,
    },

    /// A discriminating property was optional and the Discriminator Object
    /// named no `defaultMapping`.
    ///
    /// 3.2 lets the property be optional only with one
    /// (`references/3.2.0.md:3615`); whether it is optional is read as for
    /// [`DiscriminatorPropertyNotRequired`](Self::DiscriminatorPropertyNotRequired),
    /// its 3.1 counterpart. Located at the `discriminator`.
    #[cfg(feature = "openapi32")]
    #[error(
        "discriminating property `{property}` is optional, so the discriminator must declare \
         `defaultMapping`"
    )]
    DiscriminatorWithoutDefaultMapping {
        /// The `propertyName`.
        property: String,
    },

    /// A schema was deliberately left unconstrained.
    ///
    /// Located at the schema itself: one carrying the `x-kynos-unchecked`
    /// annotation wherever it is nested, or the permissive schema `true` where
    /// it is a media type's own `schema` or `itemSchema`. A `$ref` is not
    /// followed, so a component is reported once, where it is defined.
    #[error(
        "this schema is deliberately unconstrained, so the description makes no claim about \
         what it describes"
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

    /// Two routes cannot share the router's match table, which has no rule for
    /// which of them a request reaches — `/{x}.txt` beside `/v{x}`, say.
    ///
    /// Raised by a router rather than by validation: a document may hold both.
    #[error("`{pattern}` conflicts with `{existing}` in the router's match table")]
    RouteConflict {
        /// The pattern being added.
        pattern: String,
        /// The pattern already in the table.
        existing: String,
    },

    /// A matching pattern is not one the router's match table can read.
    ///
    /// Reachable only for a pattern given verbatim, such as an unchecked
    /// route's — a catch-all before the end, say. A path template is checked
    /// when it is parsed.
    #[error("`{pattern}` is not a pattern the router's match table accepts")]
    InvalidRoutePattern {
        /// The router's matching pattern, verbatim.
        pattern: String,
    },

    /// The description is opaque somewhere but does not say so at the root.
    ///
    /// The root stamp is what a consumer reads first, so omitting it is worse
    /// than being honestly incomplete.
    #[error(
        "this description contains opaque operations or routes but is not marked \
         non-authoritative"
    )]
    AuthorityNotStamped,

    /// The description is marked non-authoritative but nothing in it is
    /// opaque.
    ///
    /// The stamp is derived, so one left behind after the last opaque part was
    /// removed understates the description rather than overstating it.
    /// [`Document::restamp_authority`](crate::model::document::Document::restamp_authority)
    /// removes it.
    #[error(
        "this description is marked non-authoritative but contains no opaque operation or \
         route, so the mark is stale"
    )]
    StaleAuthorityStamp,

    /// A Kynos annotation was present but not in the shape Kynos emits.
    ///
    /// `detail` is text because the cause, a `serde_json::Error`, is neither
    /// `Clone` nor `PartialEq`.
    #[error("`{name}` is present but is not in the form Kynos emits: {detail}")]
    MalformedAnnotation {
        /// The offending field name.
        name: String,
        /// What went wrong reading it.
        detail: String,
    },

    /// The document uses 3.2-only constructs or unrecognised non-`x-` fields 3.1 cannot express.
    #[error("cannot emit as OpenAPI 3.1: {} 3.2-only or unrecognised field(s) in use: {}", blockers.len(), blockers.join(", "))]
    RequiresV3_2 {
        /// Locations of the fields standing in the way.
        blockers: Vec<String>,
    },
}
