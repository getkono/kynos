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
//! [`model::schema::dialect::OAS_DIALECT`]. It is not versioned by feature
//! flag.
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

pub mod annotation;
pub mod emit;
pub mod model;
pub mod validate;

// The curated crate-root facade. Every item below has exactly one canonical
// path inside `model` or `validate`; these shortcuts exist so that the common
// names stay one import away despite the module tree being deep.
pub use crate::{
    annotation::{MalformedAnnotation, Opaque, OpaqueReason, OpaqueRoute},
    model::{
        body::{RequestBody, encoding::Encoding, media_type::MediaType},
        callback::Callback,
        components::{ComponentName, Components},
        document::{Document, SpecVersion},
        example::{Example, ExampleValue, Examples},
        extensions::Extensions,
        external_docs::ExternalDocumentation,
        info::{Contact, Info, License},
        link::{Link, LinkTarget},
        parameter::{
            Parameter, ParameterIn, ParameterShape,
            header::{Header, HeaderShape},
            style::{HeaderStyle, Style},
        },
        paths::{
            Paths, item::PathItem, method::Method, operation::Operation, template::PathTemplate,
        },
        reference::{Ref, RefOr},
        response::{Response, Responses, status::StatusPattern},
        schema::{Schema, discriminator::Discriminator, object::SchemaObject, xml::Xml},
        security::{
            SecurityScheme,
            oauth::{OAuthFlow, OAuthFlows},
            requirement::SecurityRequirement,
        },
        server::{Server, ServerVariable},
        tag::Tag,
    },
    validate::violation::{Severity, SpecError, Violation},
};

/// The ordered map used throughout the model.
///
/// Field order in an OpenAPI description is not semantically meaningful, but
/// preserving insertion order makes emitted documents byte-stable across runs,
/// which in turn makes them reviewable in version control.
pub type Map<V> = indexmap::IndexMap<String, V>;
