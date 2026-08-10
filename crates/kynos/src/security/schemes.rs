//! The schemes Kynos can describe without being told anything.
//!
//! Each is a marker type implementing [`SecurityScheme`]. Only the schemes
//! whose description follows entirely from the scheme itself are here: an API
//! key has to say which header or cookie carries it, OAuth 2.0 has to declare
//! its flows, and OpenID Connect has to name a discovery URL, so none of the
//! three can exist as a configuration-free type. Those come from
//! `#[derive(SecurityScheme)]`, which is where the configuration goes.
//!
//! Every scheme is generic over what a verified credential yields the handler,
//! because the *description* is the same whatever that is — the document says
//! "a bearer token"; what the token means is the application's business, and
//! `Authenticates<Bearer<Claims>>` is where it says so. Without the parameter
//! an application could only ever have one bearer authenticator, and it would
//! have to hand handlers a raw `String`.

use std::marker::PhantomData;

use crate::security::SecurityScheme;

/// HTTP bearer authentication, per RFC 6750.
///
/// `bearerFormat` is an optional hint, so this describes itself completely
/// without one. Use `#[derive(SecurityScheme)]` with `bearer(format = "JWT")`
/// to add it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bearer<T = String>(PhantomData<fn() -> T>);

/// HTTP basic authentication, per RFC 7617.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Basic<T = Credentials>(PhantomData<fn() -> T>);

/// The user-id and password carried by HTTP basic authentication.
///
/// A named type rather than a pair, so that a handler signature says which
/// field is the password.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credentials {
    /// The user-id.
    pub username: String,
    /// The password.
    pub password: String,
}

/// Mutual TLS client certificate authentication.
///
/// Declared automatically when the listener is configured to verify client
/// certificates, so turning on mTLS cannot leave the description silent
/// about it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MutualTls<T = Vec<u8>>(PhantomData<fn() -> T>);

impl<T: Send + 'static> SecurityScheme for Bearer<T> {
    const NAME: &'static str = "Bearer";
    type Credential = T;

    fn describe() -> kynos_openapi::SecurityScheme {
        kynos_openapi::SecurityScheme::bearer(None)
    }

    fn challenge() -> Option<&'static str> {
        Some("Bearer")
    }
}

impl<T: Send + 'static> SecurityScheme for Basic<T> {
    const NAME: &'static str = "Basic";
    type Credential = T;

    fn describe() -> kynos_openapi::SecurityScheme {
        todo!()
    }

    fn challenge() -> Option<&'static str> {
        Some("Basic")
    }
}

impl<T: Send + 'static> SecurityScheme for MutualTls<T> {
    const NAME: &'static str = "MutualTls";
    type Credential = T;

    fn describe() -> kynos_openapi::SecurityScheme {
        todo!()
    }
}
