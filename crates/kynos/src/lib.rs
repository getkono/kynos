//! An idiomatic, performance-focused framework for building REST APIs with
//! first-class OpenAPI 3.1 and 3.2 support.
//!
//! # The idea
//!
//! Kynos only lets you build APIs it can fully describe. Every handler input
//! describes itself as a Parameter or a Request Body; every handler output
//! describes itself as a Responses Object; every interceptor declares what it
//! contributes to the description. Anything that cannot be described is
//! refused: by the compiler wherever the types can see it, otherwise by
//! [`Router::build`]. The only ways past are
//! [`Unchecked<T>`](schema::unchecked::Unchecked), which declares a payload
//! deliberately unconstrained and is annotated as such in the document, the
//! `unchecked` feature's escape hatches, named below, and `assets-fs`, which
//! implies it. Two README anti-patterns — request-derived dependencies and
//! header-based versioning — are advice the compiler does not keep.
//!
//! The emitted document is therefore a checked contract derived from the same
//! types the server runs on.
//!
//! # What this costs you
//!
//! Several things other Rust frameworks offer are absent, because OpenAPI
//! cannot express them: wildcard routes, opaque middleware, raw request access,
//! runtime-chosen status codes. WebSockets are deferred: Kynos will describe
//! the stream in an `AsyncAPI` document it emits. The `unchecked` feature
//! provides named escape hatches — `layer_unchecked`, `into_tower_unchecked`,
//! `route_unchecked` and `upgrade_unchecked` — at the price of a description
//! that is no longer authoritative for the route that took one. See the README
//! for the full list.
//!
//! # The guarantees, as compile-fail tests
//!
//! These are the claims above, stated as code that must not compile.
//!
//! A path template that is not a legal Paths key — no leading slash:
//!
//! ```compile_fail
//! kynos::path!("users/{id}");
//! ```
//!
//! A template repeating a variable, which OpenAPI 3.2 forbids outright:
//!
//! ```compile_fail
//! kynos::path!("/tenants/{id}/users/{id}");
//! ```
//!
//! A template carrying a query string, which is not part of a path:
//!
//! ```compile_fail
//! kynos::path!("/users?page=1");
//! ```
//!
//! And the one that is valid:
//!
//! ```
//! let template = kynos::path!("/users/{id}");
//! assert_eq!(template.variables(), ["id"]);
//! ```
//!
//! Further guarantees are stated where they belong: that
//! [`serde_json::Value`](crate::schema) is not describable, and that a context
//! providing no `Db` cannot satisfy [`Inject<Db>`](crate::di).
//!
//! # Feature flags
//!
//! `openapi31` is the baseline; `openapi32` is a strict superset that unlocks
//! Server-Sent Events, item streams such as JSON Lines, and whole-query-string
//! parameters, none of which OpenAPI 3.1 can describe. A streamed binary body
//! needs no flag, since 3.1 describes it as it does a buffered one. Enabling it is additive: the model
//! enums it extends are [`non_exhaustive`](openapi::SpecVersion), and the
//! structs it extends are not, so a literal or pattern of one ends in `..`. The
//! default-on `json` feature adds
//! application JSON request and response codecs; it does not control OpenAPI
//! document serialization or the framework's problem-details responses.
//!
//! The default-on `server` feature provides the `server` module. It is built on
//! tokio, which is the only supported runtime: Kynos does not abstract over the
//! runtime and offers no flag selecting another one.

// Feature badges on docs.rs, which builds on nightly with `--cfg docsrs`.
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(not(feature = "openapi31"))]
compile_error!(
    "kynos requires the `openapi31` feature. OpenAPI 3.1 is the baseline; enable \
     `openapi31`, or `openapi32`, which implies it."
);

#[cfg(all(feature = "server", not(any(feature = "http1", feature = "http2"))))]
compile_error!("the `server` feature requires at least one of `http1` or `http2`");

#[cfg(all(
    feature = "time",
    not(any(
        feature = "time-chrono04",
        feature = "time-jiff02",
        feature = "time-jiff1"
    ))
))]
compile_error!(
    "the `time` feature carries no types of its own; enable a backend: `time-chrono04`, \
     `time-jiff02` or `time-jiff1`"
);

#[cfg(all(
    feature = "decimal",
    not(any(feature = "decimal-rust1", feature = "decimal-big04"))
))]
compile_error!(
    "the `decimal` feature carries no types of its own; enable a backend, either `decimal-rust1` \
     or `decimal-big04`"
);

#[doc(hidden)]
pub mod __private;

pub mod di;
pub mod error;
pub mod extract;
pub mod handler;
pub mod http;
pub mod middleware;
pub mod response;
pub mod router;
pub mod schema;
pub mod security;

#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "test-util")]
pub mod test;

#[cfg(feature = "unchecked")]
pub mod unchecked;

/// The OpenAPI document model Kynos emits into, so a program never needs to
/// name `kynos-openapi` directly.
pub use kynos_openapi as openapi;

pub use crate::{
    error::{Error, Result, problem::Problem},
    router::{Router, endpoint::Endpoint},
};

#[cfg(feature = "macros")]
pub use kynos_macros::{
    ApiError, HeaderParams, ParamValue, PathParams, Provider, QueryParams, Reply, Schema,
    SecurityScheme, Tag, delete, get, head, name_instantiation, operation, options, patch, path,
    post, public, put, routes, trace,
};

// Gated with the trait each derives, so a missing feature reports a missing
// macro rather than an unresolved trait inside the expansion.
#[cfg(all(feature = "macros", feature = "cookie"))]
pub use kynos_macros::CookieParams;
#[cfg(all(feature = "macros", feature = "multipart"))]
pub use kynos_macros::MultipartForm;
#[cfg(all(feature = "macros", feature = "protobuf"))]
pub use kynos_macros::ProtobufMessage;
#[cfg(all(feature = "macros", feature = "assets"))]
pub use kynos_macros::assets;
#[cfg(all(feature = "macros", feature = "openapi32"))]
pub use kynos_macros::query;

/// Declares the crate's problem base: the URI prefix every problem type the
/// crate publishes hangs under, said once.
///
/// Invoked once, at the crate root, with an expression of type `&'static str`.
/// `#[problem(base = default)]` on a derived `ApiError` joins it with each
/// variant's slug, and [`problem_type!`] with the slug it is given, both in
/// `const` items, so every type URI stays a `&'static str` built at compile
/// time. A derive reads it only where it says `base = default`: one without
/// `base` still publishes `about:blank`, so the prefix stays visible on each
/// type publishing under it.
///
/// ```
/// kynos::problem_base!("https://errors.example.com/");
///
/// const INSUFFICIENT_SCOPE: &str = kynos::problem_type!("insufficient-scope");
///
/// fn main() {
///     assert_eq!(
///         INSUFFICIENT_SCOPE,
///         "https://errors.example.com/insufficient-scope"
///     );
/// }
/// ```
///
/// Anywhere but the crate root it is a compile error at the invocation, since
/// the derive and [`problem_type!`] read it from there:
///
/// ```compile_fail
/// mod problems {
///     kynos::problem_base!("https://errors.example.com/");
/// }
/// # fn main() {}
/// ```
// `crate` is the caller's on purpose: the base is the invoking crate's item.
#[expect(clippy::crate_in_macro_def)]
#[macro_export]
macro_rules! problem_base {
    ($base:expr $(,)?) => {
        #[doc(hidden)]
        pub(crate) const __KYNOS_PROBLEM_BASE: &str = $base;
        // Resolves only where the item above sits at the crate root.
        const _: &str = crate::__KYNOS_PROBLEM_BASE;
    };
}

/// The type URI the crate's [`problem_base!`] and `slug` join into, as a
/// `const` expression of type `&'static str`.
///
/// It is the URI `#[problem(base = default)]` gives a variant whose name
/// kebab-cases to `slug`, so a type URI named outside a derive, such as
/// [`Scopes::FORBIDDEN_TYPE`](crate::security::auth::Scopes::FORBIDDEN_TYPE)
/// or [`AuthRejection::forbidden_as`](crate::error::rejection::AuthRejection::forbidden_as)'s,
/// cannot drift from the prefix. `slug` is joined verbatim, as a variant's
/// `type` is sent.
///
/// ```
/// use kynos::error::rejection::AuthRejection;
///
/// kynos::problem_base!("https://errors.example.com/");
///
/// const DEACTIVATED: AuthRejection =
///     AuthRejection::forbidden_as(kynos::problem_type!("account-deactivated"));
///
/// fn main() {
///     assert_eq!(
///         kynos::problem_type!("account-deactivated"),
///         "https://errors.example.com/account-deactivated"
///     );
///     # let _ = DEACTIVATED;
/// }
/// ```
///
/// A crate declaring no [`problem_base!`] has no prefix to join:
///
/// ```compile_fail
/// const INSUFFICIENT_SCOPE: &str = kynos::problem_type!("insufficient-scope");
/// # fn main() {}
/// ```
// `crate` is the caller's on purpose, as in `problem_base!`.
#[expect(clippy::crate_in_macro_def)]
#[macro_export]
macro_rules! problem_type {
    ($slug:literal $(,)?) => {{
        const __KYNOS_PROBLEM_LEN: usize = crate::__KYNOS_PROBLEM_BASE.len() + $slug.len();
        const __KYNOS_PROBLEM_BYTES: &[u8] = &$crate::__private::problem::join::<
            __KYNOS_PROBLEM_LEN,
        >(crate::__KYNOS_PROBLEM_BASE, $slug);
        // A `const`, so the UTF-8 check runs once, at compile time.
        const __KYNOS_PROBLEM_URI: &str = $crate::__private::problem::utf8(__KYNOS_PROBLEM_BYTES);
        __KYNOS_PROBLEM_URI
    }};
}

/// Everything a typical application needs, in one import.
pub mod prelude {
    pub use crate::{
        di::inject::Inject,
        error::{Error, Result, problem::Problem},
        // Each extractor beside the derive it is used with.
        extract::params::{header::Headers, path::Path, query::Query},
        response::{
            headers::WithHeaders,
            status::{Accepted, Created, Location, NoContent, Redirect},
        },
        router::{Router, group::Group},
        // Unaliased: rustc names a trait by its shortest visible path in
        // diagnostics. The derive shares the name from the macro namespace.
        schema::{ParamValue, Schema},
        security::auth::{Auth, Scoped},
    };

    #[cfg(feature = "json")]
    pub use crate::extract::body::json::Json;

    #[cfg(feature = "macros")]
    pub use crate::{
        ApiError, HeaderParams, ParamValue, PathParams, Provider, QueryParams, Reply, Schema,
        SecurityScheme, Tag, delete, get, head, operation, options, patch, path, post, put, routes,
        trace,
    };

    #[cfg(feature = "cookie")]
    pub use crate::extract::params::cookie::Cookies;

    #[cfg(all(feature = "macros", feature = "cookie"))]
    pub use crate::CookieParams;

    #[cfg(all(feature = "macros", feature = "multipart"))]
    pub use crate::MultipartForm;

    #[cfg(all(feature = "macros", feature = "protobuf"))]
    pub use crate::ProtobufMessage;

    #[cfg(feature = "server")]
    pub use crate::server::Server;
}
