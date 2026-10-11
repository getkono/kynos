//! The schemes Kynos can describe without being told anything.
//!
//! Each is a marker type implementing [`SecurityScheme`] whose description
//! follows entirely from the scheme itself. An API key, OAuth 2.0 and OpenID
//! Connect need configuration, so they come from `#[derive(SecurityScheme)]`.
//!
//! Every scheme is generic over what a verified credential yields the handler,
//! which the description does not depend on: `Authenticates<Bearer<Claims>>`
//! is where an application says what its token means. Each is generic over its
//! [`Scope`] too, [`Unscoped`] unless named: `Bearer<Claims, Grant>` is a
//! bearer token whose operations may demand `Grant`s, and is described exactly
//! as `Bearer<Claims>` is.

use std::marker::PhantomData;

use crate::{
    error::rejection::AuthRejection,
    http::Parts,
    security::{
        Scope, SecurityScheme, Unscoped,
        carrier::{self, BearerToken, Carries, Credentials, PeerCertificates},
    },
};

/// HTTP bearer authentication, per RFC 6750.
///
/// `bearerFormat` is an optional hint, so this describes itself completely
/// without one. Use `#[derive(SecurityScheme)]` with `bearer(format = "JWT")`
/// to add it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bearer<T = String, P = Unscoped>(PhantomData<fn() -> (T, P)>);

/// HTTP basic authentication, per RFC 7617.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Basic<T = Credentials, P = Unscoped>(PhantomData<fn() -> (T, P)>);

/// Mutual TLS client certificate authentication.
///
/// Declared automatically when the listener is configured to verify client
/// certificates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MutualTls<T = Vec<u8>, P = Unscoped>(PhantomData<fn() -> (T, P)>);

impl<T: Send + 'static, P: Scope> SecurityScheme for Bearer<T, P> {
    const NAME: &'static str = "Bearer";
    type Credential = T;
    type Scope = P;

    fn describe() -> kynos_openapi::SecurityScheme {
        kynos_openapi::SecurityScheme::bearer(None)
    }

    fn challenge() -> Option<&'static str> {
        Some("Bearer")
    }
}

impl<T: Send + 'static, P: Scope> SecurityScheme for Basic<T, P> {
    const NAME: &'static str = "Basic";
    type Credential = T;
    type Scope = P;

    fn describe() -> kynos_openapi::SecurityScheme {
        kynos_openapi::SecurityScheme::basic()
    }

    /// RFC 7617 section 2: `charset` is what tells a client to send a non-ASCII
    /// password as UTF-8, and `UTF-8` is the only value the registry defines.
    ///
    /// No `realm`, whose value a deployment chooses. A scheme needing it
    /// declares its own challenge through `#[derive(SecurityScheme)]`, as
    /// `examples/security_schemes.rs` shows.
    fn challenge() -> Option<&'static str> {
        Some(r#"Basic charset="UTF-8""#)
    }
}

// No `challenge`: no HTTP authentication scheme is registered for a
// certificate presented during the TLS handshake.
impl<T: Send + 'static, P: Scope> SecurityScheme for MutualTls<T, P> {
    const NAME: &'static str = "MutualTls";
    type Credential = T;
    type Scope = P;

    fn describe() -> kynos_openapi::SecurityScheme {
        kynos_openapi::SecurityScheme::mutual_tls()
    }
}

/// Each scheme's carrier is the one its own description implies: `bearer` and
/// `basic` are `Authorization` schemes, and a client certificate is presented
/// during the handshake rather than in any field.
impl<T: Send + 'static, P: Scope> Carries for Bearer<T, P> {
    type Presented = BearerToken;

    fn present(parts: &Parts) -> Result<Option<BearerToken>, AuthRejection> {
        carrier::bearer(parts)
    }
}

impl<T: Send + 'static, P: Scope> Carries for Basic<T, P> {
    type Presented = Credentials;

    fn present(parts: &Parts) -> Result<Option<Credentials>, AuthRejection> {
        carrier::basic(parts)
    }
}

impl<T: Send + 'static, P: Scope> Carries for MutualTls<T, P> {
    type Presented = PeerCertificates;

    fn present(parts: &Parts) -> Result<Option<PeerCertificates>, AuthRejection> {
        carrier::peer_certificates(parts)
    }
}

#[cfg(test)]
mod tests;
