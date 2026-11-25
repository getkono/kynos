//! What a router reports wrong with itself.

/// What is wrong with a router, as one of its
/// [`Violation`](kynos_openapi::Violation)s says.
///
/// A router is checked twice over: its description against the OpenAPI
/// specification, which reports a [`SpecError`](kynos_openapi::SpecError), and
/// itself, for what no document can hold — its match table, its interceptors,
/// the routers it absorbed. One report carries both, so a violation found in
/// the description arrives wrapped in [`Spec`](RouterError::Spec).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RouterError {
    /// The description fails to conform to the specification.
    #[error(transparent)]
    Spec(#[from] kynos_openapi::SpecError),

    /// One method on one path was described more than once.
    ///
    /// A Path Item holds one Operation per method, so no document can carry
    /// the second: the router reports it while it assembles the description.
    #[error(
        "operation `{method} {path}` is described more than once; a path item holds one \
         operation per method"
    )]
    DuplicateOperation {
        /// The method described twice.
        method: kynos_openapi::Method,
        /// The `paths` key it was described under.
        path: String,
    },

    /// A `GET` describes the `HEAD` it answers on a path that also declares a
    /// `HEAD` operation of its own.
    ///
    /// The declared `HEAD` answers that method, so the one the `GET` would
    /// describe never would. Reported rather than settled by either winning:
    /// both were asked for, and one of them is a mistake. Located at the
    /// `head` operation.
    #[error(
        "`HEAD {path}` is declared as an operation and also described by its `GET`'s `head`; \
         remove one of them"
    )]
    HeadDescribedTwice {
        /// The `paths` key both are at.
        path: String,
    },

    /// An operation that is not a `GET` describes a `HEAD`, which only a
    /// `GET` answers (RFC 9110 section 9.3.2).
    ///
    /// Reachable only through
    /// [`EndpointBuilder::describe_head`](crate::router::endpoint::builder::EndpointBuilder::describe_head),
    /// whose method is a runtime value; the route attribute refuses its `head`
    /// argument anywhere but on a `GET`.
    #[error(
        "operation `{method} {path}` describes a `HEAD`, which only a `GET` answers; remove \
         `describe_head`"
    )]
    HeadWithoutGet {
        /// The operation's method.
        method: kynos_openapi::Method,
        /// The `paths` key it is at.
        path: String,
    },

    /// An operation takes no guard while another operation of its
    /// [`Group`](crate::router::group::Group) takes one, and does not declare
    /// that it is public.
    ///
    /// A guard is each handler's own argument, so a handler that leaves it out
    /// compiles and is served to anyone, and its operation is described
    /// without `security`. Among guarded siblings that is a forgotten guard
    /// far more often than an intent. An operation meant to be reached
    /// without a credential says so with `#[kynos::public]`, or
    /// [`EndpointBuilder::public`](crate::router::endpoint::builder::EndpointBuilder::public).
    ///
    /// Reported once per such operation, located at it.
    #[error(
        "operation `{method} {path}` takes no guard while another operation in its group takes \
         one; take the guard, or mark the operation `#[kynos::public]` if it is meant to be \
         reached without a credential"
    )]
    UnguardedOperation {
        /// The operation's method.
        method: kynos_openapi::Method,
        /// The `paths` key it is at.
        path: String,
    },

    /// An interceptor's short circuit declared statuses its responses do not
    /// describe, or the reverse.
    ///
    /// Only reachable from a hand-written
    /// [`ShortCircuit`](crate::response::ShortCircuit); the one the `ApiError`
    /// derive emits comes from the same declaration as the responses and
    /// cannot disagree with them.
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

    /// Two routes cannot share the router's match table, which has no rule for
    /// which of them a request reaches — `/{x}.txt` beside `/v{x}`, say.
    ///
    /// A document may hold both; only the table refuses them.
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

    /// A router that was nested or merged into another set an
    /// application-wide policy, which only the root router's applies.
    ///
    /// Reported once per policy set and located at the root: the absorbed
    /// router's value would otherwise be dropped without a word.
    #[error(
        "a nested or merged router sets `{policy}`, which only the root router's applies; \
         set it on the root router instead"
    )]
    NestedRouterPolicy {
        /// The setter the absorbed router called, as the router spells it
        /// (`trusted_proxies`).
        policy: &'static str,
    },

    /// The members of the policy [`problems`](crate::Router::problems) names
    /// do not describe an object naming each member as a property, so no
    /// problem could carry them beside its own.
    ///
    /// A struct deriving `Schema` describes one; a `#[serde(flatten)]`, an
    /// enum or a newtype does not.
    #[error("problem members `{members}` must describe an object naming each member as a property")]
    ProblemMembersNotObject {
        /// The type's name, as `std::any::type_name` spells it.
        members: &'static str,
    },

    /// The members of the policy [`problems`](crate::Router::problems) names
    /// declare a member a problem Kynos renders already sends: one RFC 9457
    /// registers, or `errors`.
    #[error(
        "problem members `{members}` declare `{member}`, which a problem Kynos renders already \
         sends; rename it"
    )]
    ProblemMemberReserved {
        /// The type's name, as `std::any::type_name` spells it.
        members: &'static str,
        /// The member, as the type's schema names it.
        member: String,
    },

    /// A [`map_openapi`](crate::Router::map_openapi) edit changed which
    /// operation the document describes at a method and path, where the
    /// router serves one, or what the router serves it by.
    ///
    /// The router mounts the operations and dispatches by what the document
    /// says of them, so an edit may describe one differently but not add,
    /// remove, move or rename (`operationId`) one, add or remove its security
    /// requirement (which keeps the response cache from sharing what a guard
    /// admitted), change the `Accept-Query` its `QUERY` body derives (which its
    /// 415 sends), or hide that it is unverified. Located at the operation as the router assembled it, or as
    /// the edit added it.
    #[error(
        "a `map_openapi` edit changed operation `{method} {path}`; an edit may describe an \
         operation differently, but not add, remove, move or rename it, add or remove its \
         security requirement, change the media types its `QUERY` body accepts, or remove its \
         `x-kynos-opaque` marker"
    )]
    MappedOperation {
        /// The operation's method.
        method: kynos_openapi::Method,
        /// The `paths` key it is at.
        path: String,
    },

    /// A `#[schema(string)]` type lists an example it does not carry as that
    /// string, so the document would show a reader a value the API refuses.
    ///
    /// Found while the type's schema is described, which cannot fail, and
    /// reported once per example, located at it under the type's component.
    #[error("`{schema}` lists the example {example:?}, which {refusal}")]
    StringExample {
        /// The type, as `std::any::type_name` spells it.
        schema: &'static str,
        /// The example, as listed.
        example: String,
        /// Why the type does not carry it.
        refusal: ExampleRefusal,
    },

    /// A query parameter's `#[param(default = ...)]` breaks the bounds its
    /// field or its type declares, so the document would state a default the
    /// API refuses when a client sends it, and the decoder would bind a value
    /// no request could carry.
    ///
    /// Found while the group describes its parameters, which cannot fail, and
    /// located at each operation taking the group.
    #[error(
        "`{group}` defaults the query parameter `{parameter}` to {default:?}, which breaks its \
         bounds: {}",
        failures.join("; ")
    )]
    ParameterDefault {
        /// The group, as `std::any::type_name` spells it.
        group: &'static str,
        /// The parameter's wire name.
        parameter: String,
        /// The default, as its type's `Display` writes it.
        default: String,
        /// What each broken bound says.
        failures: Vec<String>,
    },

    /// A query parameter group names one parameter twice, through a
    /// `#[param(flatten)]` group: its own field and a flattened group's, or
    /// two flattened groups'.
    ///
    /// Both fields would read the one pair naming it, and the description
    /// can carry it once. Found while the group describes its parameters,
    /// since only the flattened group knows its names, and located at each
    /// operation taking the group.
    #[error(
        "`{group}` names the query parameter `{parameter}` twice, through a flattened group; \
         rename one of them"
    )]
    DuplicateQueryParameter {
        /// The group flattening the other, as `std::any::type_name` spells
        /// it.
        group: &'static str,
        /// The parameter's wire name.
        parameter: String,
    },
}

/// Why a `#[schema(string)]` type does not carry an example it lists, as a
/// [`StringExample`](RouterError::StringExample) violation says.
///
/// The serde refusals apply wherever the type implements serde, however it is
/// used: the schema is described with no knowledge of its uses, and every use
/// names the one component, a JSON body included. A type whose serde serves
/// another purpose keeps that serde on a separate type.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ExampleRefusal {
    /// The type's own `FromStr` refuses the example.
    #[error("its own `FromStr` refuses")]
    Unparsed,

    /// The value `FromStr` reads breaks the bounds the type declares on its
    /// text, checked on what its `Display` writes.
    #[error("breaks the type's own bounds: {}", failures.join("; "))]
    OutOfBounds {
        /// What each broken bound says.
        failures: Vec<String>,
    },

    /// The type's `Serialize` writes the value `FromStr` reads as something
    /// other than the JSON string its `Display` writes, as a serde derive
    /// carrying no attribute writes a newtype's member or a struct's fields.
    #[error(
        "its `Serialize` writes as {written}, not as the string its `Display` writes, \
         {displayed:?}; carry it through the text, with `#[serde(into = \"String\", \
         try_from = \"String\")]` or serde written by hand"
    )]
    Miswritten {
        /// The JSON serde wrote.
        written: String,
        /// The text `Display` wrote.
        displayed: String,
    },

    /// The type's `Serialize` refuses the value `FromStr` reads.
    #[error("its `Serialize` refuses to write: {reason}")]
    Unserializable {
        /// What serde said.
        reason: String,
    },

    /// The type's `Deserialize` does not read the example as a JSON string.
    #[error("its `Deserialize` does not read as a JSON string: {reason}")]
    Undeserializable {
        /// What serde said.
        reason: String,
    },
}
