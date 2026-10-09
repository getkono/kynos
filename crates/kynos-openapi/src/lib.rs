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
//! Fields introduced by 3.2 are `#[cfg]`-gated, so a build without `openapi32`
//! cannot construct a document it could not describe. To get a 3.1 document
//! from a build with `openapi32` enabled, use [`Document::emit`], which fails
//! with the 3.2-only constructs that block the downgrade.
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

// docs.rs badges each feature-gated item; see `crates/kynos/src/lib.rs`.
#![cfg_attr(docsrs, feature(doc_cfg))]

// `openapi31` is the baseline object model; `openapi32` implies it, so this
// fires only when default features are disabled and neither is asked for.
#[cfg(not(feature = "openapi31"))]
compile_error!(
    "kynos-openapi requires the `openapi31` feature. OpenAPI 3.1 is the baseline object model; \
     enable `openapi31`, or `openapi32`, which implies it."
);

pub mod annotation;
pub mod emit;
pub mod model;
#[cfg(feature = "pattern")]
pub mod pattern;
pub mod validate;

// The curated crate-root facade: shortcuts to items whose canonical paths are
// inside `annotation`, `model` or `validate`.
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
            style::{EncodingStyle, HeaderStyle, Style},
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
/// Preserving insertion order keeps emitted documents byte-stable across runs.
pub type Map<V> = indexmap::IndexMap<String, V>;
