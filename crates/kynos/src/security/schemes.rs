//! The schemes Kynos knows how to describe.
//!
//! Each is a unit struct implementing [`SecurityScheme`]; `#[derive(SecurityScheme)]`
//! exists for the cases these do not cover, such as an API key under a
//! non-standard header name.

use crate::security::SecurityScheme;

/// HTTP bearer authentication, per RFC 6750.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bearer;

/// HTTP basic authentication, per RFC 7617.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Basic;

/// An API key carried in a header, query parameter or cookie.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApiKey;

/// OAuth 2.0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OAuth2;

/// OpenID Connect Discovery.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpenIdConnect;

/// Mutual TLS client certificate authentication.
///
/// Declared automatically when the listener is configured to verify client
/// certificates, so turning on mTLS cannot leave the description silent
/// about it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MutualTls;

impl SecurityScheme for Bearer {
    const NAME: &'static str = "Bearer";
    type Credential = String;

    fn describe() -> kynos_openapi::SecurityScheme {
        todo!()
    }
}

impl SecurityScheme for Basic {
    const NAME: &'static str = "Basic";
    type Credential = (String, String);

    fn describe() -> kynos_openapi::SecurityScheme {
        todo!()
    }
}

impl SecurityScheme for ApiKey {
    const NAME: &'static str = "ApiKey";
    type Credential = String;

    fn describe() -> kynos_openapi::SecurityScheme {
        todo!()
    }
}

impl SecurityScheme for OAuth2 {
    const NAME: &'static str = "OAuth2";
    type Credential = String;

    fn describe() -> kynos_openapi::SecurityScheme {
        todo!()
    }
}

impl SecurityScheme for OpenIdConnect {
    const NAME: &'static str = "OpenIdConnect";
    type Credential = String;

    fn describe() -> kynos_openapi::SecurityScheme {
        todo!()
    }
}

impl SecurityScheme for MutualTls {
    const NAME: &'static str = "MutualTls";
    type Credential = Vec<u8>;

    fn describe() -> kynos_openapi::SecurityScheme {
        todo!()
    }
}
