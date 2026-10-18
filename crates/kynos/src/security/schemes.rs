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
///
/// `R` is the [`Realm`] its challenge names, [`ApiRealm`] unless named:
/// `Basic<Credentials, Unscoped, Admin>` challenges with `realm="admin"` for
/// an `Admin` whose [`Realm::REALM`] is `"admin"`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Basic<T = Credentials, P = Unscoped, R = ApiRealm>(Markers<T, P, R>);

/// [`Basic`]'s parameters, owning none of them.
type Markers<T, P, R> = PhantomData<fn() -> (T, P, R)>;

/// The protection space a [`Basic`] challenge names.
///
/// RFC 7617 section 2 makes `realm` REQUIRED on every Basic challenge; a
/// client keys the credentials it remembers by it. The challenge sends
/// [`REALM`](Self::REALM) as a quoted-string, escaping `"` and `\`.
///
/// A realm a header cannot carry fails the build wherever the scheme is used:
///
/// ```compile_fail
/// use kynos::security::{SecurityScheme, Unscoped, carrier::Credentials, schemes::{Basic, Realm}};
///
/// struct Broken;
///
/// impl Realm for Broken {
///     const REALM: &'static str = "line\nbreak";
/// }
///
/// let _ = <Basic<Credentials, Unscoped, Broken> as SecurityScheme>::challenge();
/// ```
///
/// Its control, differing only in the realm:
///
/// ```
/// use kynos::security::{SecurityScheme, Unscoped, carrier::Credentials, schemes::{Basic, Realm}};
///
/// struct Admin;
///
/// impl Realm for Admin {
///     const REALM: &'static str = "admin";
/// }
///
/// assert_eq!(
///     <Basic<Credentials, Unscoped, Admin> as SecurityScheme>::challenge(),
///     Some(r#"Basic realm="admin", charset="UTF-8""#),
/// );
/// ```
pub trait Realm: 'static {
    /// The realm, unquoted: visible ASCII, space and tab, at most 225 bytes
    /// once escaped.
    const REALM: &'static str;
}

/// The realm [`Basic`] names unless told another: `api`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApiRealm;

impl Realm for ApiRealm {
    const REALM: &'static str = "api";
}

/// The challenge's bytes before the realm.
const CHALLENGE_PREFIX: &[u8] = br#"Basic realm=""#;

/// The challenge's bytes after the realm. RFC 7617 section 2: `charset` is
/// what tells a client to send a non-ASCII password as UTF-8, and `UTF-8` is
/// the only value the registry defines.
const CHALLENGE_SUFFIX: &[u8] = br#"", charset="UTF-8""#;

/// The bytes a composed challenge may occupy.
const CHALLENGE_CAPACITY: usize = 256;

/// The challenge naming `R`, composed once per realm at compile time.
struct Challenge<R>(PhantomData<R>);

impl<R: Realm> Challenge<R> {
    const COMPOSED: ([u8; CHALLENGE_CAPACITY], usize) = compose(R::REALM);
    const BYTES: &'static [u8; CHALLENGE_CAPACITY] = &Self::COMPOSED.0;
    const VALUE: &'static str = match core::str::from_utf8(Self::BYTES.split_at(Self::COMPOSED.1).0)
    {
        Ok(value) => value,
        Err(_) => panic!("a composed challenge is ASCII"),
    };
}

/// `Basic realm="<realm>", charset="UTF-8"`, and its length.
///
/// # Panics
///
/// Where `realm` holds a byte outside RFC 9110's `qdtext` and `quoted-pair`
/// (a control, DEL or non-ASCII, which a header value cannot carry), or does
/// not fit [`CHALLENGE_CAPACITY`] once composed. Evaluated in a constant, either is
/// a build error.
const fn compose(realm: &str) -> ([u8; CHALLENGE_CAPACITY], usize) {
    let mut out = [0; CHALLENGE_CAPACITY];
    let mut len = append(&mut out, 0, CHALLENGE_PREFIX);

    let realm = realm.as_bytes();
    let mut index = 0;
    while index < realm.len() {
        let byte = realm[index];
        assert!(
            byte == b'\t' || matches!(byte, b' '..=b'~'),
            "a realm is visible ASCII, space and tab: a header value cannot carry anything else"
        );
        if byte == b'"' || byte == b'\\' {
            len = append(&mut out, len, b"\\");
        }
        len = append(&mut out, len, &[byte]);
        index += 1;
    }

    len = append(&mut out, len, CHALLENGE_SUFFIX);
    (out, len)
}

/// Copies `bytes` into `out` at `len`, returning the new length.
const fn append(out: &mut [u8; CHALLENGE_CAPACITY], mut len: usize, bytes: &[u8]) -> usize {
    assert!(
        len + bytes.len() <= CHALLENGE_CAPACITY,
        "a realm is at most 225 bytes once escaped"
    );
    let mut index = 0;
    while index < bytes.len() {
        out[len] = bytes[index];
        len += 1;
        index += 1;
    }
    len
}

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

impl<T: Send + 'static, P: Scope, R: Realm> SecurityScheme for Basic<T, P, R> {
    const NAME: &'static str = "Basic";
    type Credential = T;
    type Scope = P;

    fn describe() -> kynos_openapi::SecurityScheme {
        kynos_openapi::SecurityScheme::basic()
    }

    /// `Basic realm="<R::REALM>", charset="UTF-8"`: RFC 7617 section 2
    /// requires the realm, and `charset` tells a client to send a non-ASCII
    /// password as UTF-8.
    fn challenge() -> Option<&'static str> {
        Some(Challenge::<R>::VALUE)
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

impl<T: Send + 'static, P: Scope, R: Realm> Carries for Basic<T, P, R> {
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
