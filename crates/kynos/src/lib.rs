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
//! # Feature flags
//!
//! `openapi31` is the baseline; `openapi32` is a strict superset that unlocks
//! Server-Sent Events, streaming bodies and whole-query-string parameters,
//! none of which OpenAPI 3.1 can describe.

// `openapi31` is the baseline object model rather than an optional extra.
// `openapi32` implies it, so this fires only when a caller disables default
// features and asks for neither.
#[cfg(not(feature = "openapi31"))]
compile_error!(
    "kynos requires the `openapi31` feature. OpenAPI 3.1 is the baseline; enable \
     `openapi31`, or `openapi32`, which implies it."
);

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
    error::{Error, Problem, Result},
    router::{Endpoint, Router},
};

#[cfg(feature = "macros")]
pub use kynos_macros::{
    ApiError, Cookies, Headers, PathParams, Provider, QueryParams, Reply, Schema, SecurityScheme,
    Tag, delete, get, head, operation, options, patch, path, post, put, routes, trace,
};

#[cfg(all(feature = "macros", feature = "openapi32"))]
pub use kynos_macros::query;

/// Everything a typical application needs, in one import.
pub mod prelude {
    pub use crate::{
        di::Inject,
        error::{Error, Problem, Result},
        extract::{Json, Path, Query},
        response::{Created, NoContent},
        router::{Group, Router},
        schema::Schema as SchemaTrait,
    };

    #[cfg(feature = "macros")]
    pub use crate::{
        ApiError, Cookies, Headers, PathParams, Provider, QueryParams, Reply, Schema,
        SecurityScheme, Tag, delete, get, head, operation, options, patch, path, post, put, routes,
        trace,
    };

    #[cfg(feature = "server")]
    pub use crate::server::Server;
}
