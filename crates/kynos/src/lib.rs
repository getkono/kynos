//! An idiomatic, performance-focused framework for building REST APIs with
//! first-class OpenAPI 3.1 and 3.2 support.
//!
//! # The idea
//!
//! Kynos only lets you build APIs it can fully describe. Every handler input
//! describes itself as a Parameter or a Request Body; every handler output
//! describes itself as a Responses Object; every interceptor declares what it
//! contributes to the description. Anything that cannot be described does not
//! compile.
//!
//! The emitted document is therefore not documentation that drifts from the
//! code — it is a checked contract derived from the same types the server runs
//! on.
//!
//! # What this costs you
//!
//! Several things other Rust frameworks offer are absent, because OpenAPI
//! cannot express them: wildcard routes, opaque middleware, raw request access,
//! runtime-chosen status codes, WebSockets. The `unchecked` feature provides
//! escape hatches for the first three, at the price of a description that is no
//! longer authoritative. See the README for the full list and the reasoning.
//!
//! # The guarantees, as compile-fail tests
//!
//! These are the claims above, stated as code that must not compile. They run
//! as part of the doc test suite, so a regression in any of them is a test
//! failure rather than a documentation inaccuracy.
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
//! Server-Sent Events, streaming bodies and whole-query-string parameters,
//! none of which OpenAPI 3.1 can describe. The default-on `json` feature adds
//! application JSON request and response codecs; it does not control OpenAPI
//! document serialization or the framework's problem-details responses.
//!
//! The default-on `server` feature provides the `server` module. It is built on
//! tokio, which is the only supported runtime: Kynos does not abstract over the
//! runtime and offers no flag selecting another one.

// `openapi31` is the baseline object model rather than an optional extra.
// `openapi32` implies it, so this fires only when a caller disables default
// features and asks for neither.
#[cfg(not(feature = "openapi31"))]
compile_error!(
    "kynos requires the `openapi31` feature. OpenAPI 3.1 is the baseline; enable \
     `openapi31`, or `openapi32`, which implies it."
);

#[cfg(all(feature = "server", not(any(feature = "http1", feature = "http2"))))]
compile_error!("the `server` feature requires at least one of `http1` or `http2`");

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

/// The OpenAPI document model Kynos emits into.
///
/// Re-exported so that a program depending on `kynos` never needs to name
/// `kynos-openapi` directly.
pub use kynos_openapi as openapi;

pub use crate::{
    error::{Error, Result, problem::Problem},
    router::{Router, endpoint::Endpoint},
};

#[cfg(feature = "macros")]
pub use kynos_macros::{
    ApiError, Headers, PathParams, Provider, QueryParams, Reply, Schema, SecurityScheme, Tag,
    delete, get, head, operation, options, patch, path, post, put, routes, trace,
};

// Each of these derives a trait that its feature gates, so exporting it more
// widely would only trade one diagnostic for a worse one: "no derive macro
// named `Cookies`" says which feature to enable, where "cannot find trait
// `CookieParams`" points at an expansion the user did not write.
#[cfg(all(feature = "macros", feature = "cookie"))]
pub use kynos_macros::Cookies;
#[cfg(all(feature = "macros", feature = "openapi32"))]
pub use kynos_macros::query;

/// Everything a typical application needs, in one import.
pub mod prelude {
    pub use crate::{
        di::inject::Inject,
        error::{Error, Result, problem::Problem},
        extract::params::{path::Path, query::Query},
        response::status::{Created, NoContent},
        router::{Router, group::Group},
        schema::Schema as SchemaTrait,
    };

    #[cfg(feature = "json")]
    pub use crate::extract::body::json::Json;

    #[cfg(feature = "macros")]
    pub use crate::{
        ApiError, Headers, PathParams, Provider, QueryParams, Reply, Schema, SecurityScheme, Tag,
        delete, get, head, operation, options, patch, path, post, put, routes, trace,
    };

    #[cfg(all(feature = "macros", feature = "cookie"))]
    pub use crate::Cookies;

    #[cfg(feature = "server")]
    pub use crate::server::Server;
}
