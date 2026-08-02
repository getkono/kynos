//! The OpenAPI 3.1 and 3.2 document model.
//!
//! This crate is the data model Kynos emits into, and is deliberately free of
//! any runtime dependency: no `tokio`, no `hyper`. It can be used on its own to
//! build, serialize, or validate an OpenAPI description.
//!
//! # Specification versions
//!
//! `openapi31` is the baseline and is enabled by default. `openapi32` adds the
//! fields introduced by OpenAPI 3.2.0 as a strict superset.
//!
//! Fields introduced by 3.2 are `#[cfg]`-gated rather than runtime-optional, so
//! a build without `openapi32` cannot construct a document it would be unable
//! to describe. Where a program needs a 3.1 document from a build that has
//! `openapi32` enabled — Cargo unifies features across a dependency graph, so
//! this is not always the program's own choice — use [`Document::emit`], which
//! fails with the list of 3.2-only constructs that block the downgrade rather
//! than silently emitting an invalid description.
//!
//! # A note on the JSON Schema dialect
//!
//! OpenAPI 3.2 did *not* mint a new JSON Schema dialect. Both 3.1 and 3.2 use
//! `https://spec.openapis.org/oas/3.1/dialect/base`, exposed here as
//! [`schema::OAS_DIALECT`]. It is not versioned by feature flag.
//!
//! # Example
//!
//! ```
//! use kynos_openapi::{Document, Info, SpecVersion};
//!
//! let doc = Document::new(SpecVersion::V3_1, Info::new("Orders", "1.0.0"));
//! let json = doc.to_json().expect("serializable");
//! assert!(json.contains("\"openapi\""));
//! ```

// `openapi31` is the baseline object model, not an optional extra: without it
// there is nothing to build a description out of. `openapi32` implies it, so
// this fires only when a caller disables default features and asks for neither.
#[cfg(not(feature = "openapi31"))]
compile_error!(
    "kynos-openapi requires the `openapi31` feature. OpenAPI 3.1 is the baseline object model; \
     enable `openapi31`, or `openapi32`, which implies it."
);

pub mod body;
pub mod callback;
pub mod components;
pub mod document;
pub mod example;
pub mod extensions;
pub mod external_docs;
pub mod info;
pub mod link;
pub mod parameter;
pub mod paths;
pub mod reference;
pub mod response;
pub mod schema;
pub mod security;
pub mod server;
pub mod tag;
pub mod validate;

pub use crate::{
    body::{Encoding, MediaType, RequestBody},
    callback::Callback,
    components::{ComponentName, Components},
    document::{Document, SpecVersion},
    example::Example,
    extensions::Extensions,
    external_docs::ExternalDocumentation,
    info::{Contact, Info, License},
    link::Link,
    parameter::{Header, Parameter, ParameterIn, Style},
    paths::{Method, Operation, PathItem, PathTemplate, Paths},
    reference::{Ref, RefOr},
    response::{Response, Responses, StatusPattern},
    schema::{Discriminator, Schema, SchemaObject, Xml},
    security::{OAuthFlow, OAuthFlows, SecurityRequirement, SecurityScheme},
    server::{Server, ServerVariable},
    tag::Tag,
    validate::{Severity, SpecError, Violation},
};

/// The ordered map used throughout the model.
///
/// Field order in an OpenAPI description is not semantically meaningful, but
/// preserving insertion order makes emitted documents byte-stable across runs,
/// which in turn makes them reviewable in version control.
pub type Map<V> = indexmap::IndexMap<String, V>;
