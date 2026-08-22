//! The OAuth Flows and OAuth Flow Objects.

use serde::{Deserialize, Serialize};

use crate::{Map, model::extensions::Extensions};

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

impl OAuthFlows {
    /// Adds the implicit flow.
    #[must_use]
    pub fn with_implicit(mut self, flow: OAuthFlow) -> Self {
        self.implicit = Some(flow);
        self
    }

    /// Adds the resource owner password credentials flow.
    #[must_use]
    pub fn with_password(mut self, flow: OAuthFlow) -> Self {
        self.password = Some(flow);
        self
    }

    /// Adds the client credentials flow.
    #[must_use]
    pub fn with_client_credentials(mut self, flow: OAuthFlow) -> Self {
        self.client_credentials = Some(flow);
        self
    }

    /// Adds the authorization code flow.
    #[must_use]
    pub fn with_authorization_code(mut self, flow: OAuthFlow) -> Self {
        self.authorization_code = Some(flow);
        self
    }

    /// Adds the RFC 8628 device authorization flow.
    #[cfg(feature = "openapi32")]
    #[must_use]
    pub fn with_device_authorization(mut self, flow: OAuthFlow) -> Self {
        self.device_authorization = Some(flow);
        self
    }
}

/// The configuration of one OAuth 2.0 flow.
///
/// Which URL fields are required depends on the flow this is attached to. That
/// pairing is *not* checked here, and the model is deliberately the permissive
/// half: a flow read back from someone else's description has to round-trip
/// whatever it said. `#[derive(SecurityScheme)]` enforces the pairing where the
/// flow is written, which is the layer that knows which flow it is naming.
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

    /// Sets the device authorization URL.
    ///
    /// Required for the device authorization flow, and legal on any of them.
    #[cfg(feature = "openapi32")]
    #[must_use]
    pub fn with_device_authorization_url(mut self, url: impl Into<String>) -> Self {
        self.device_authorization_url = Some(url.into());
        self
    }
}
