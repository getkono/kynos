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
}
