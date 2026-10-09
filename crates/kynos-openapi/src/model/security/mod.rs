//! The Security Scheme, OAuth Flows and Security Requirement Objects.

pub mod oauth;
pub mod requirement;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{extensions::Extensions, parameter::ParameterIn, security::oauth::OAuthFlows};

/// A security scheme the API can use.
///
/// One variant per `type` value the specification defines, so an unusable
/// combination — an `apiKey` scheme with OAuth flows, say — cannot be
/// constructed. The enum and every variant are `#[non_exhaustive]`, since
/// `openapi32` adds fields and Cargo unifies features across a build.
///
/// So a pattern takes `..`, and reads the same in either build:
///
/// ```
/// # use kynos_openapi::SecurityScheme;
/// fn scheme_of(security: &SecurityScheme) -> Option<&str> {
///     match security {
///         SecurityScheme::Http { scheme, .. } => Some(scheme),
///         _ => None,
///     }
/// }
/// # assert_eq!(scheme_of(&SecurityScheme::basic()), Some("basic"));
/// ```
///
/// Naming every field is a compile error even when the list is complete for
/// this build:
///
/// ```compile_fail
/// # use kynos_openapi::SecurityScheme;
/// fn scheme_of(security: &SecurityScheme) -> Option<&str> {
///     match security {
///         SecurityScheme::Http {
///             scheme,
///             bearer_format,
///             description,
///             deprecated,
///             extensions,
///         } => Some(scheme),
///         _ => None,
///     }
/// }
/// ```
///
/// Construct through [`http`](Self::http), [`bearer`](Self::bearer),
/// [`basic`](Self::basic), the three `api_key_*`, [`mutual_tls`](Self::mutual_tls),
/// [`oauth2`](Self::oauth2) and [`open_id_connect`](Self::open_id_connect),
/// then [`with_description`](Self::with_description),
/// [`with_extension`](Self::with_extension) and the rest.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SecurityScheme {
    /// A key carried in a header, query parameter or cookie.
    #[non_exhaustive]
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
    #[non_exhaustive]
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
    /// Kynos declares this automatically when the listener verifies client
    /// certificates.
    #[non_exhaustive]
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
    #[non_exhaustive]
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
    #[non_exhaustive]
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
    /// An HTTP authentication scheme, named by its RFC 7235 scheme token.
    ///
    /// For schemes other than [`bearer`](Self::bearer) and [`basic`](Self::basic).
    #[must_use]
    pub fn http(scheme: impl Into<String>, bearer_format: Option<String>) -> Self {
        Self::Http {
            scheme: scheme.into(),
            bearer_format,
            description: None,
            #[cfg(feature = "openapi32")]
            deprecated: None,
            extensions: Extensions::new(),
        }
    }

    /// An HTTP bearer token scheme.
    #[must_use]
    pub fn bearer(bearer_format: Option<String>) -> Self {
        Self::http("bearer", bearer_format)
    }

    /// An HTTP basic authentication scheme.
    #[must_use]
    pub fn basic() -> Self {
        Self::http("basic", None)
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

    /// An API key carried in a query parameter.
    pub fn api_key_query(name: impl Into<String>) -> Self {
        Self::ApiKey {
            name: name.into(),
            location: ParameterIn::Query,
            description: None,
            #[cfg(feature = "openapi32")]
            deprecated: None,
            extensions: Extensions::new(),
        }
    }

    /// An API key carried in a cookie.
    pub fn api_key_cookie(name: impl Into<String>) -> Self {
        Self::ApiKey {
            name: name.into(),
            location: ParameterIn::Cookie,
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

    /// OAuth 2.0 with the given flows.
    #[must_use]
    pub fn oauth2(flows: OAuthFlows) -> Self {
        Self::OAuth2 {
            flows: Box::new(flows),
            #[cfg(feature = "openapi32")]
            oauth2_metadata_url: None,
            description: None,
            #[cfg(feature = "openapi32")]
            deprecated: None,
            extensions: Extensions::new(),
        }
    }

    /// OpenID Connect Discovery, against the given metadata URL.
    pub fn open_id_connect(url: impl Into<String>) -> Self {
        Self::OpenIdConnect {
            open_id_connect_url: url.into(),
            description: None,
            #[cfg(feature = "openapi32")]
            deprecated: None,
            extensions: Extensions::new(),
        }
    }

    /// Sets the scheme's description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        let slot = match &mut self {
            Self::ApiKey { description, .. }
            | Self::Http { description, .. }
            | Self::MutualTls { description, .. }
            | Self::OAuth2 { description, .. }
            | Self::OpenIdConnect { description, .. } => description,
        };
        *slot = Some(description.into());
        self
    }

    /// Attaches a specification extension.
    ///
    /// Read them back with a pattern binding `extensions` and `..`.
    #[must_use]
    pub fn with_extension(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        let slot = match &mut self {
            Self::ApiKey { extensions, .. }
            | Self::Http { extensions, .. }
            | Self::MutualTls { extensions, .. }
            | Self::OAuth2 { extensions, .. }
            | Self::OpenIdConnect { extensions, .. } => extensions,
        };
        slot.insert(key, value);
        self
    }

    /// States whether the scheme is deprecated.
    ///
    /// Unlike [`deprecated`](Self::deprecated), this can state `false`.
    ///
    /// Introduced in OpenAPI 3.2, and a blocker for emitting the document as
    /// 3.1 — see [`emit`](crate::emit).
    #[cfg(feature = "openapi32")]
    #[must_use]
    pub fn with_deprecated(mut self, deprecated: bool) -> Self {
        let slot = match &mut self {
            Self::ApiKey { deprecated, .. }
            | Self::Http { deprecated, .. }
            | Self::MutualTls { deprecated, .. }
            | Self::OAuth2 { deprecated, .. }
            | Self::OpenIdConnect { deprecated, .. } => deprecated,
        };
        *slot = Some(deprecated);
        self
    }

    /// Marks the scheme deprecated.
    ///
    /// Introduced in OpenAPI 3.2, and a blocker for emitting the document as
    /// 3.1 — see [`emit`](crate::emit).
    #[cfg(feature = "openapi32")]
    #[must_use]
    pub fn deprecated(mut self) -> Self {
        let slot = match &mut self {
            Self::ApiKey { deprecated, .. }
            | Self::Http { deprecated, .. }
            | Self::MutualTls { deprecated, .. }
            | Self::OAuth2 { deprecated, .. }
            | Self::OpenIdConnect { deprecated, .. } => deprecated,
        };
        *slot = Some(true);
        self
    }

    /// Sets the RFC 8414 authorization server metadata URL.
    ///
    /// Ignored by any scheme that is not OAuth 2.0. Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    #[must_use]
    pub fn with_oauth2_metadata_url(mut self, url: impl Into<String>) -> Self {
        if let Self::OAuth2 {
            oauth2_metadata_url,
            ..
        } = &mut self
        {
            *oauth2_metadata_url = Some(url.into());
        }
        self
    }
}

#[cfg(test)]
mod tests;
