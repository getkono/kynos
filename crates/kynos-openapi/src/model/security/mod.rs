//! The Security Scheme, OAuth Flows and Security Requirement Objects.

pub mod oauth;
pub mod requirement;

use serde::{Deserialize, Serialize};

use crate::model::{extensions::Extensions, parameter::ParameterIn, security::oauth::OAuthFlows};

/// A security scheme the API can use.
///
/// The variants are the five `type` values the specification defines. Modelling
/// them as an enum rather than one struct with conditionally-required fields
/// means an unusable combination — an `apiKey` scheme with OAuth flows, say —
/// cannot be constructed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SecurityScheme {
    /// A key carried in a header, query parameter or cookie.
    #[serde(rename = "apiKey")]
    ApiKey {
        /// The name of the header, query parameter or cookie.
        name: String,
        /// Where the key is carried. Only query, header and cookie are legal.
        #[serde(rename = "in")]
        location: ParameterIn,
        /// A description of the scheme. [CommonMark] syntax may be used.
        ///
        /// [CommonMark]: https://spec.commonmark.org/
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Whether the scheme is deprecated.
        ///
        /// Introduced in OpenAPI 3.2.
        #[cfg(feature = "openapi32")]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deprecated: Option<bool>,
        /// Specification extensions.
        #[serde(flatten)]
        extensions: Extensions,
    },

    /// An RFC 7235 `Authorization` header scheme.
    #[serde(rename = "http")]
    Http {
        /// The registered authorization scheme name, such as `bearer`.
        scheme: String,
        /// A hint about the bearer token's format, such as `JWT`.
        #[serde(
            rename = "bearerFormat",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        bearer_format: Option<String>,
        /// A description of the scheme.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Whether the scheme is deprecated.
        ///
        /// Introduced in OpenAPI 3.2.
        #[cfg(feature = "openapi32")]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deprecated: Option<bool>,
        /// Specification extensions.
        #[serde(flatten)]
        extensions: Extensions,
    },

    /// Mutual TLS client certificate authentication.
    ///
    /// Kynos declares this automatically when the listener is configured to
    /// verify client certificates, so enabling mTLS cannot leave the
    /// description silent about it.
    #[serde(rename = "mutualTLS")]
    MutualTls {
        /// A description of the scheme.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Whether the scheme is deprecated.
        ///
        /// Introduced in OpenAPI 3.2.
        #[cfg(feature = "openapi32")]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deprecated: Option<bool>,
        /// Specification extensions.
        #[serde(flatten)]
        extensions: Extensions,
    },

    /// OAuth 2.0.
    #[serde(rename = "oauth2")]
    OAuth2 {
        /// The supported flows.
        flows: Box<OAuthFlows>,
        /// A URL to the RFC 8414 authorization server metadata.
        ///
        /// Introduced in OpenAPI 3.2.
        #[cfg(feature = "openapi32")]
        #[serde(
            rename = "oauth2MetadataUrl",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        oauth2_metadata_url: Option<String>,
        /// A description of the scheme.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Whether the scheme is deprecated.
        ///
        /// Introduced in OpenAPI 3.2.
        #[cfg(feature = "openapi32")]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deprecated: Option<bool>,
        /// Specification extensions.
        #[serde(flatten)]
        extensions: Extensions,
    },

    /// OpenID Connect Discovery.
    #[serde(rename = "openIdConnect")]
    OpenIdConnect {
        /// The OpenID Connect Discovery URL.
        #[serde(rename = "openIdConnectUrl")]
        open_id_connect_url: String,
        /// A description of the scheme.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Whether the scheme is deprecated.
        ///
        /// Introduced in OpenAPI 3.2.
        #[cfg(feature = "openapi32")]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deprecated: Option<bool>,
        /// Specification extensions.
        #[serde(flatten)]
        extensions: Extensions,
    },
}

impl SecurityScheme {
    /// An HTTP bearer token scheme.
    #[must_use]
    pub fn bearer(bearer_format: Option<String>) -> Self {
        Self::Http {
            scheme: "bearer".to_owned(),
            bearer_format,
            description: None,
            #[cfg(feature = "openapi32")]
            deprecated: None,
            extensions: Extensions::new(),
        }
    }

    /// An HTTP basic authentication scheme.
    #[must_use]
    pub fn basic() -> Self {
        Self::Http {
            scheme: "basic".to_owned(),
            bearer_format: None,
            description: None,
            #[cfg(feature = "openapi32")]
            deprecated: None,
            extensions: Extensions::new(),
        }
    }

    /// An API key carried in a header.
    pub fn api_key_header(name: impl Into<String>) -> Self {
        Self::ApiKey {
            name: name.into(),
            location: ParameterIn::Header,
            description: None,
            #[cfg(feature = "openapi32")]
            deprecated: None,
            extensions: Extensions::new(),
        }
    }

    /// Mutual TLS client certificate authentication.
    #[must_use]
    pub fn mutual_tls() -> Self {
        Self::MutualTls {
            description: None,
            #[cfg(feature = "openapi32")]
            deprecated: None,
            extensions: Extensions::new(),
        }
    }
}

#[cfg(test)]
mod tests;
