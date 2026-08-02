//! The Security Scheme, OAuth Flows and Security Requirement Objects.

use serde::{Deserialize, Serialize};

use crate::{Map, extensions::Extensions, parameter::ParameterIn};

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

/// The OAuth 2.0 flows a scheme supports.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthFlows {
    /// The implicit flow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implicit: Option<OAuthFlow>,

    /// The resource owner password credentials flow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<OAuthFlow>,

    /// The client credentials flow.
    #[serde(
        rename = "clientCredentials",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub client_credentials: Option<OAuthFlow>,

    /// The authorization code flow.
    #[serde(
        rename = "authorizationCode",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub authorization_code: Option<OAuthFlow>,

    /// The RFC 8628 device authorization flow.
    ///
    /// Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    #[serde(
        rename = "deviceAuthorization",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub device_authorization: Option<OAuthFlow>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

/// The configuration of one OAuth 2.0 flow.
///
/// Which URL fields are required depends on the flow this is attached to;
/// [`crate::validate`] checks the pairing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthFlow {
    /// The authorization URL. Required for the implicit and authorization code
    /// flows.
    #[serde(
        rename = "authorizationUrl",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub authorization_url: Option<String>,

    /// The token URL. Required for the password, client credentials and
    /// authorization code flows.
    #[serde(rename = "tokenUrl", default, skip_serializing_if = "Option::is_none")]
    pub token_url: Option<String>,

    /// The device authorization URL. Required for the device authorization
    /// flow.
    ///
    /// Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    #[serde(
        rename = "deviceAuthorizationUrl",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub device_authorization_url: Option<String>,

    /// The URL used to obtain refresh tokens.
    #[serde(
        rename = "refreshUrl",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub refresh_url: Option<String>,

    /// The scopes available, mapped to a short description of each.
    ///
    /// Required, though it may be empty.
    pub scopes: Map<String>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl OAuthFlow {
    /// Creates a flow with the given scopes and no URLs.
    pub fn new(scopes: impl IntoIterator<Item = (String, String)>) -> Self {
        Self {
            scopes: scopes.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Sets the authorization URL.
    #[must_use]
    pub fn with_authorization_url(mut self, url: impl Into<String>) -> Self {
        self.authorization_url = Some(url.into());
        self
    }

    /// Sets the token URL.
    #[must_use]
    pub fn with_token_url(mut self, url: impl Into<String>) -> Self {
        self.token_url = Some(url.into());
        self
    }

    /// Sets the refresh URL.
    #[must_use]
    pub fn with_refresh_url(mut self, url: impl Into<String>) -> Self {
        self.refresh_url = Some(url.into());
        self
    }
}

/// The security schemes that must be satisfied to invoke an operation.
///
/// Each key names a scheme in
/// [`Components::security_schemes`](crate::Components::security_schemes); the
/// value lists the required scopes, which is meaningful only for `oauth2` and
/// `openIdConnect`. All entries in one requirement must be satisfied together;
/// a list of requirements is satisfied when any one of them is.
///
/// An empty requirement means anonymous access is permitted.
///
/// This object carries no extensions: the specification does not permit them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecurityRequirement(pub Map<Vec<String>>);

impl SecurityRequirement {
    /// Creates a requirement permitting anonymous access.
    #[must_use]
    pub fn anonymous() -> Self {
        Self(Map::new())
    }

    /// Requires a scheme that takes no scopes.
    pub fn scheme(name: impl Into<String>) -> Self {
        let mut map = Map::new();
        map.insert(name.into(), Vec::new());
        Self(map)
    }

    /// Requires a scheme together with a set of scopes.
    pub fn scoped(
        name: impl Into<String>,
        scopes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let mut map = Map::new();
        map.insert(name.into(), scopes.into_iter().map(Into::into).collect());
        Self(map)
    }

    /// Adds another scheme that must be satisfied alongside the existing ones.
    #[must_use]
    pub fn and(
        mut self,
        name: impl Into<String>,
        scopes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.0
            .insert(name.into(), scopes.into_iter().map(Into::into).collect());
        self
    }

    /// Returns `true` when this requirement permits anonymous access.
    #[must_use]
    pub fn is_anonymous(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{SecurityRequirement, SecurityScheme};

    #[test]
    fn the_scheme_type_is_the_serde_tag() {
        let json = serde_json::to_string(&SecurityScheme::bearer(Some("JWT".to_owned())))
            .expect("serializable");
        assert!(json.contains(r#""type":"http""#));
        assert!(json.contains(r#""scheme":"bearer""#));
        assert!(json.contains(r#""bearerFormat":"JWT""#));
    }

    #[test]
    fn mutual_tls_needs_no_further_configuration() {
        let json = serde_json::to_string(&SecurityScheme::mutual_tls()).expect("serializable");
        assert_eq!(json, r#"{"type":"mutualTLS"}"#);
    }

    #[test]
    fn an_empty_requirement_means_anonymous_access() {
        assert!(SecurityRequirement::anonymous().is_anonymous());
        assert!(!SecurityRequirement::scheme("Bearer").is_anonymous());
    }

    #[test]
    fn requirements_serialize_as_a_bare_map() {
        let json = serde_json::to_string(&SecurityRequirement::scoped("OAuth", ["read", "write"]))
            .expect("serializable");
        assert_eq!(json, r#"{"OAuth":["read","write"]}"#);
    }
}
