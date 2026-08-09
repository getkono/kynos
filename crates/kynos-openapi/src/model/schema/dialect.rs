//! The JSON Schema dialect OpenAPI descriptions are written in.

/// The JSON Schema dialect used by OpenAPI.
///
/// OpenAPI 3.2 did **not** mint a new dialect: 3.1 and 3.2 share this URI. It
/// is therefore not gated by feature flag.
pub const OAS_DIALECT: &str = "https://spec.openapis.org/oas/3.1/dialect/base";
