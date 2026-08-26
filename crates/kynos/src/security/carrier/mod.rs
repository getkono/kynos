//! Where a credential travels, read from the same text that documents it.
//!
//! A [`SecurityScheme`] already says how a credential is *carried* — `bearer`
//! puts it in `Authorization`, `api_key(in = "cookie", name = "session")` names
//! a cookie. [`Carries`] is that same statement made executable, so the field an
//! authenticator reads and the field the description advertises are one string
//! rather than two that agree today.
//!
//! # Why this is a trait and not a helper
//!
//! Handing an [`Authenticator`](crate::security::Authenticator) a `&Parts` and
//! a shelf of parsing functions would leave the drift exactly where it was: the
//! description would say `X-Api-Key` and the verifier would be free to read
//! `X-API-Token`, and nothing would notice. Taking
//! [`Presented`](Carries::Presented) instead means an authenticator *cannot*
//! read anything the scheme did not declare, because it is never given the
//! request.
//!
//! That is the difference from every framework that configures a "finder"
//! beside the documentation. There is nothing to keep in step here, because
//! there is only one statement.
//!
//! # What is deliberately not read
//!
//! RFC 6750 defines three ways to present a bearer token and Kynos reads one.
//! The form-encoded body (section 2.2) is not reachable from a request *head*,
//! and making a credential a body field would put it in the operation's schema.
//! The URI query parameter (section 2.3) is `SHOULD NOT` in the RFC itself, and
//! for good reason: a token in a query string reaches access logs, `Referer`
//! headers and browser history. Neither is a gap to close later.

pub(super) mod base64;
mod parse;

use crate::{
    error::rejection::AuthRejection,
    http::{Parts, header::AUTHORIZATION},
    security::SecurityScheme,
};

/// A security scheme that says where its credential travels.
///
/// Derived along with [`SecurityScheme`] itself: the same `#[security(...)]`
/// attribute that writes the description writes this, so the two cannot come
/// apart. Implement it by hand only for a scheme whose carrier the grammar
/// cannot express, and reach for the readers in this module when you do.
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not say where its credential is carried",
    label = "no declared carrier",
    note = "derive `SecurityScheme`: the same `#[security(...)]` attribute that writes the \
            description also writes the carrier, so the two cannot disagree",
    note = "implement it by hand only for a carrier the attribute grammar cannot express, and \
            build it from the readers in `kynos::security::carrier`"
)]
pub trait Carries: SecurityScheme {
    /// The credential as the request presented it, before anything verified it.
    ///
    /// Owned rather than borrowed from the request head. A borrowing form would
    /// put a lifetime parameter into every application's `impl Authenticator`,
    /// which `docs/architecture.md` rules out for the public surface: generics
    /// that exist for performance stay private. The cost is one allocation per
    /// authenticated request, against a verifier that is about to check a
    /// signature or read a session store.
    type Presented: Send;

    /// Reads the presented credential out of the request head.
    ///
    /// Three answers, not two. `Ok(None)` is *absent*, which is anonymity and
    /// which [`MaybeAuth`](crate::security::auth::MaybeAuth) treats as such.
    /// `Err` is *present and malformed*, which is a 401 even there: a client
    /// that sent a broken credential is not an anonymous client.
    ///
    /// The challenge is left unset. [`Auth`](crate::security::auth::Auth)
    /// attaches the scheme's own on the way out, so the string on the wire and
    /// the string the operation declares cannot be different ones.
    fn present(parts: &Parts) -> Result<Option<Self::Presented>, AuthRejection>;
}

/// A bearer token, as RFC 6750 section 2.1 presented it.
///
/// Opaque by definition: the token's meaning is the issuer's business, and a
/// type that claimed to know it would be claiming more than it can check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BearerToken(String);

impl BearerToken {
    /// The token, exactly as the client sent it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Takes ownership of the token.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

/// The credentials an arbitrary RFC 9110 authentication scheme carried.
///
/// What `http(scheme = "...")` yields, for a scheme with no wire form Kynos
/// models — `Negotiate`, `HOBA`, or one an application invented. The scheme
/// token is here as well as the credentials, since a scheme that admits more
/// than one spelling may need to know which arrived.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemeCredentials {
    scheme: String,
    credentials: String,
}

impl SchemeCredentials {
    /// The scheme token, as the client spelled it.
    #[must_use]
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    /// Everything after the scheme token.
    #[must_use]
    pub fn credentials(&self) -> &str {
        &self.credentials
    }
}

/// Where an API key travels.
///
/// The three the specification permits, and the three
/// `#[derive(SecurityScheme)]` accepts. A key in the path is part of the URL
/// rather than a credential, so it is absent here as it is there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum KeyLocation {
    /// A request header field of the service's choosing.
    Header,
    /// A query parameter.
    Query,
    /// A cookie.
    ///
    /// Not gated on the `cookie` feature. That feature names a *dependency* and
    /// the parameter extractor built on it; reading one field out of a jar
    /// needs neither, and a scheme that describes a credential in a cookie has
    /// to be able to read one wherever it is described.
    Cookie,
}

/// An API key, read from the field its scheme declared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// The key, exactly as the client sent it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Takes ownership of the key.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

/// Reads a bearer token, per RFC 6750 section 2.1.
///
/// The scheme token is matched case-insensitively, per RFC 9110 section 11.1.
///
/// # Errors
///
/// When an `Authorization` field is present and is not a bearer credential.
pub fn bearer(parts: &Parts) -> Result<Option<BearerToken>, AuthRejection> {
    let Some(authorization) = parse::authorization(parts)? else {
        return Ok(None);
    };

    // A different scheme is not this scheme's credential. Refusing rather than
    // reporting absence is deliberate: the client did present something, and
    // calling that anonymous would let `MaybeAuth` wave through a request
    // carrying a credential nobody checked.
    if !parse::scheme_is(authorization.scheme, "bearer") {
        return Err(AuthRejection::unauthenticated());
    }

    if authorization.credentials.is_empty() {
        return Err(AuthRejection::unauthenticated());
    }

    Ok(Some(BearerToken(authorization.credentials.to_owned())))
}

/// Reads a user-id and password, per RFC 7617.
///
/// The credentials are base64, and the user-id is everything before the *first*
/// colon — a password may hold one, a user-id may not.
///
/// # Errors
///
/// When an `Authorization` field is present and is not a well-formed basic
/// credential: a different scheme, base64 that does not decode, bytes that are
/// not UTF-8, or no colon at all.
pub fn basic(parts: &Parts) -> Result<Option<super::schemes::Credentials>, AuthRejection> {
    let Some(authorization) = parse::authorization(parts)? else {
        return Ok(None);
    };

    if !parse::scheme_is(authorization.scheme, "basic") {
        return Err(AuthRejection::unauthenticated());
    }

    let decoded =
        base64::decode(authorization.credentials).ok_or_else(AuthRejection::unauthenticated)?;

    // RFC 7617 section 2.1 says the charset is whatever the challenge named, and
    // `charset="UTF-8"` is the only value the registry defines. Anything else is
    // a credential this service did not ask for.
    let text = String::from_utf8(decoded).map_err(|_| AuthRejection::unauthenticated())?;

    // The *first* colon: a password may contain one, a user-id may not.
    let (username, password) = text
        .split_once(':')
        .ok_or_else(AuthRejection::unauthenticated)?;

    Ok(Some(super::schemes::Credentials {
        username: username.to_owned(),
        password: password.to_owned(),
    }))
}

/// Reads the credentials of an arbitrary RFC 9110 authentication scheme.
///
/// # Errors
///
/// When an `Authorization` field is present and names a different scheme.
pub fn http_scheme(
    parts: &Parts,
    scheme: &str,
) -> Result<Option<SchemeCredentials>, AuthRejection> {
    let Some(authorization) = parse::authorization(parts)? else {
        return Ok(None);
    };

    if !parse::scheme_is(authorization.scheme, scheme) {
        return Err(AuthRejection::unauthenticated());
    }

    Ok(Some(SchemeCredentials {
        scheme: authorization.scheme.to_owned(),
        credentials: authorization.credentials.to_owned(),
    }))
}

/// Reads an API key from the field its scheme declared.
///
/// # Errors
///
/// When the field is present and holds bytes no `&str` can carry.
pub fn api_key(
    parts: &Parts,
    location: KeyLocation,
    name: &str,
) -> Result<Option<ApiKey>, AuthRejection> {
    match location {
        KeyLocation::Header => {
            // `Authorization` is refused as an API key field at compile time,
            // so a key read here never collides with one of the schemes above.
            debug_assert!(
                !name.eq_ignore_ascii_case(AUTHORIZATION.as_str()),
                "`#[derive(SecurityScheme)]` refuses `Authorization` as an API key field"
            );

            let Some(field) = parts.headers.get(name) else {
                return Ok(None);
            };
            let value = field
                .to_str()
                .map_err(|_| AuthRejection::unauthenticated())?;
            Ok(Some(ApiKey(value.to_owned())))
        }

        KeyLocation::Query => Ok(query_value(parts, name).map(|value| ApiKey(value.into_owned()))),

        KeyLocation::Cookie => Ok(crate::http::cookie::value_of(&parts.headers, name)
            .map(|value| ApiKey(value.to_owned()))),
    }
}

/// The first value of `name` in the request target's query string.
///
/// Percent-decoded, since a key is a value rather than a piece of the URL's
/// syntax; owned when decoding changed something, borrowed when it did not. The
/// decoder is the one [`Query`](crate::extract::params::query::Query) already
/// reaches for, so a key and a parameter cannot disagree about an escape.
///
/// `+` is a plus sign rather than a space. That is `application/x-www-form-
/// urlencoded`'s rule and a query string is not a form; a key containing a
/// literal `+` is far likelier than one whose issuer form-encoded a space into
/// it.
fn query_value<'r>(parts: &'r Parts, name: &str) -> Option<std::borrow::Cow<'r, str>> {
    parts.uri.query()?.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        // A name is percent-encoded too, so comparing raw bytes would miss the
        // client that wrote `api%5Fkey`.
        let key = crate::__private::uri::decode_path_value(key).ok()?;
        (key == name)
            .then(|| crate::__private::uri::decode_path_value(value).ok())
            .flatten()
    })
}

/// The certificate chain the peer presented during the TLS handshake.
///
/// The one credential no request field carries, which is exactly why the scheme
/// has to be declared: nothing else about the request reveals it.
#[derive(Clone, Debug)]
pub struct PeerCertificates(crate::extract::connection::Connection);

impl PeerCertificates {
    /// The chain, DER, leaf first.
    #[must_use]
    pub fn chain(&self) -> &[bytes::Bytes] {
        self.0.peer_certificates()
    }

    /// The certificate the peer is identified by.
    #[must_use]
    pub fn leaf(&self) -> Option<&bytes::Bytes> {
        self.chain().first()
    }

    /// The server name the client asked for through SNI.
    ///
    /// Which certificate authority a chain is checked against often depends on
    /// which name the client was talking to.
    #[must_use]
    pub fn server_name(&self) -> Option<&str> {
        self.0.server_name()
    }
}

/// Reads the peer's certificate chain.
///
/// `Ok(None)` for every way of not having presented one: the connection is not
/// TLS, the listener does not verify client certificates, the peer sent none,
/// or this build has no `tls` feature. None of the four is distinguishable to a
/// client, so none is distinguished here.
///
/// # Why this is not gated on `tls`
///
/// A guard would key on the wrong thing. A service behind a TLS-terminating
/// proxy sees no certificates *with* the feature on, so the feature does not
/// answer "can this deployment authenticate a client certificate" — the
/// deployment does. Making `Auth<MutualTls>` a compile error without `tls`
/// would be a precision Kynos cannot actually offer, and it would stop
/// `examples/security_schemes.rs` showing the scheme at all.
///
/// # Errors
///
/// Never. The signature matches every other reader here so that a derived
/// [`Carries`] needs no special case for this one.
pub fn peer_certificates(parts: &Parts) -> Result<Option<PeerCertificates>, AuthRejection> {
    let Some(connection) = parts
        .extensions
        .get::<crate::extract::connection::Connection>()
    else {
        return Ok(None);
    };
    if connection.peer_certificates().is_empty() {
        return Ok(None);
    }
    Ok(Some(PeerCertificates(connection.clone())))
}

#[cfg(test)]
mod tests;
