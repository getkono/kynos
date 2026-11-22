//! Where a credential travels, read from the same text that documents it.
//!
//! A [`SecurityScheme`] already says how a credential is *carried* — `bearer`
//! puts it in `Authorization`, `api_key(in = "cookie", name = "session")` names
//! a cookie. [`Carries`] is that same statement made executable, so the field an
//! authenticator reads and the field the description advertises are one string.
//! An [`Authenticator`](crate::security::Authenticator) receives the credential
//! as [`Presented`], read by the scheme's own carrier, so it never finds the
//! credential itself.
//!
//! # What is deliberately not read
//!
//! Of RFC 6750's three ways to present a bearer token, Kynos reads the header
//! one. The form-encoded body (section 2.2) is not reachable from a request
//! *head*, and the URI query parameter (section 2.3) is `SHOULD NOT` in the RFC
//! itself.

mod base64;
mod parse;
pub mod presented;
mod query;
mod redact;

use crate::{
    http::{Parts, header::AUTHORIZATION},
    security::SecurityScheme,
};
use presented::{Malformed, MalformedReason, Presented};

/// A security scheme that says where its credential travels.
///
/// Derived along with [`SecurityScheme`] itself: the same `#[security(...)]`
/// attribute that writes the description writes this, so the two cannot come
/// apart. Implement it by hand only for a scheme whose carrier the grammar
/// cannot express, and reach for the readers in this module when you do.
///
/// A carrier that reads request headers beside its credential declares them
/// through [`SecurityScheme::reads`], which `#[security(..., reads = T)]`
/// writes, so every operation a guard naming the scheme covers describes them.
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
    /// Owned rather than borrowed from the request head, so an
    /// `impl Authenticator` carries no lifetime parameter.
    type Presented: Send;

    /// Whether an [`Absent`](Presented::Absent) credential reaches the
    /// [`Authenticator`](crate::security::Authenticator) rather than being
    /// answered by the guard.
    ///
    /// `false` by default: absence is [`Auth`](crate::security::auth::Auth)'s
    /// 401 and [`MaybeAuth`](crate::security::auth::MaybeAuth)'s anonymity.
    /// `#[security(..., carrier = optional)]` sets it, for a deployment that
    /// may run with no credentials configured, which only the authenticator
    /// knows. Nothing then reports absence to the guard, so `MaybeAuth` never
    /// sees anonymity and an [`AnyOf`](crate::security::requirement::AnyOf)
    /// never skips the scheme.
    const OPTIONAL: bool = false;

    /// Reads the presented credential out of the request head.
    ///
    /// Classifies, never refuses: what [`Presented`] says is the
    /// authenticator's to judge, but for absence, which the guard answers
    /// unless [`OPTIONAL`](Self::OPTIONAL) is set.
    fn present(parts: &Parts) -> Presented<Self::Presented>;
}

/// A bearer token, as RFC 6750 section 2.1 presented it.
///
/// Opaque: the token's meaning is the issuer's business.
#[derive(Clone, PartialEq, Eq)]
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

/// The username and password HTTP basic authentication carried, per RFC 7617,
/// which calls the username the `user-id`.
///
/// A named type rather than a pair, so that a handler signature says which
/// field is the password.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    username: String,
    password: String,
}

impl Credentials {
    /// The username: everything before the first colon.
    #[must_use]
    pub fn username(&self) -> &str {
        &self.username
    }

    /// The password: everything after the first colon, which may hold more.
    #[must_use]
    pub fn password(&self) -> &str {
        &self.password
    }

    /// Takes ownership of the username and password, in that order.
    #[must_use]
    pub fn into_parts(self) -> (String, String) {
        (self.username, self.password)
    }
}

/// The credentials an arbitrary RFC 9110 authentication scheme carried.
///
/// What `http(scheme = "...")` yields, for a scheme with no wire form Kynos
/// models — `Negotiate`, `HOBA`, or one an application invented. The scheme
/// token is here as well as the credentials, since a scheme that admits more
/// than one spelling may need to know which arrived.
#[derive(Clone, PartialEq, Eq)]
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
    /// Not gated on the `cookie` feature, which names a dependency this reader
    /// does not need.
    Cookie,
}

/// An API key, read from the field its scheme declared.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// The key: the value of its field, decoded as that field's location
    /// decodes a value.
    ///
    /// A header value is as the client sent it; a cookie value has its
    /// surrounding whitespace and double quotes removed; a query value is
    /// decoded by the form rules of every `in: query` parameter, so `a+b`
    /// arrives as `a b` and `a%2Bb` as `a+b`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Takes ownership of the key, decoded as [`as_str`](Self::as_str)
    /// describes.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

/// Reads a bearer token, per RFC 6750 section 2.1.
///
/// The scheme token is matched case-insensitively, per RFC 9110 section 11.1.
///
/// [`Malformed`](Presented::Malformed) when an `Authorization` field is present
/// and is not a bearer credential: two such fields, a value that is not
/// visible ASCII, an empty scheme, a different scheme, no token, or a token
/// outside the `b64token` grammar.
pub fn bearer(parts: &Parts) -> Presented<BearerToken> {
    parse::authorization(parts).and_then(|authorization| {
        // Another scheme's credential is malformed, not absent, so `MaybeAuth`
        // does not wave through a credential nobody checked.
        if !parse::scheme_is(authorization.scheme, "bearer") {
            return malformed(MalformedReason::OtherScheme);
        }
        if !is_b64token(authorization.credentials) {
            return malformed(MalformedReason::BadCredentials);
        }
        Presented::Present(BearerToken(authorization.credentials.to_owned()))
    })
}

/// A malformed credential, for `reason`.
fn malformed<T>(reason: MalformedReason) -> Presented<T> {
    Presented::Malformed(Malformed::new(reason))
}

/// Whether `token` is an RFC 6750 section 2.1 `b64token`:
/// `1*( ALPHA / DIGIT / "-" / "." / "_" / "~" / "+" / "/" ) *"="`.
fn is_b64token(token: &str) -> bool {
    let body = token.trim_end_matches('=');
    !body.is_empty()
        && body.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'+' | b'/')
        })
}

/// Reads a user-id and password, per RFC 7617.
///
/// The credentials are base64, and the user-id is everything before the *first*
/// colon — a password may hold one, a user-id may not.
///
/// [`Malformed`](Presented::Malformed) when an `Authorization` field is present
/// and is not a well-formed basic credential: two such fields, a value that is
/// not visible ASCII, an empty scheme, a different scheme, base64 that does not
/// decode, bytes that are not UTF-8, or no colon at all.
pub fn basic(parts: &Parts) -> Presented<Credentials> {
    parse::authorization(parts).and_then(|authorization| {
        if !parse::scheme_is(authorization.scheme, "basic") {
            return malformed(MalformedReason::OtherScheme);
        }
        let Some(decoded) = base64::decode(authorization.credentials) else {
            return malformed(MalformedReason::BadCredentials);
        };
        // RFC 7617 section 2.1: `charset="UTF-8"` is the only charset defined.
        let Ok(text) = String::from_utf8(decoded) else {
            return malformed(MalformedReason::BadCredentials);
        };
        let Some((username, password)) = text.split_once(':') else {
            return malformed(MalformedReason::BadCredentials);
        };
        Presented::Present(Credentials {
            username: username.to_owned(),
            password: password.to_owned(),
        })
    })
}

/// Reads the credentials of an arbitrary RFC 9110 authentication scheme.
///
/// [`Malformed`](Presented::Malformed) when an `Authorization` field is present
/// and is not a credential of `scheme`: two such fields, a value that is not
/// visible ASCII, an empty scheme, or a different scheme.
pub fn http_scheme(parts: &Parts, scheme: &str) -> Presented<SchemeCredentials> {
    parse::authorization(parts).and_then(|authorization| {
        if !parse::scheme_is(authorization.scheme, scheme) {
            return malformed(MalformedReason::OtherScheme);
        }
        Presented::Present(SchemeCredentials {
            scheme: authorization.scheme.to_owned(),
            credentials: authorization.credentials.to_owned(),
        })
    })
}

/// Reads an API key from the field its scheme declared.
///
/// [`Malformed`](Presented::Malformed) when `location` is
/// [`KeyLocation::Header`] and `name` is `Authorization` in any case, whether
/// or not the request carries it: that field belongs to the HTTP
/// authentication schemes above, so it is never an API key.
///
/// [`Malformed`](Presented::Malformed) too when the key is present and cannot
/// be read: a header field sent twice, a header field or the first cookie of
/// that name whose value is not ASCII, or a first query pair of that name whose
/// percent-decoded value is not UTF-8. A later pair or cookie never stands in
/// for the first.
pub fn api_key(parts: &Parts, location: KeyLocation, name: &str) -> Presented<ApiKey> {
    match location {
        KeyLocation::Header => {
            // `#[derive(SecurityScheme)]` refuses this at compile time; a
            // hand-written `Carries` reaches it here, in every build.
            if name.eq_ignore_ascii_case(AUTHORIZATION.as_str()) {
                return malformed(MalformedReason::ReservedField);
            }

            let mut fields = parts.headers.get_all(name).into_iter();
            let Some(field) = fields.next() else {
                return Presented::Absent;
            };
            // A key is one value, so two fields are refused as two
            // `Authorization` fields are: choosing either would let an
            // appending proxy pick the credential.
            if fields.next().is_some() {
                return malformed(MalformedReason::Repeated);
            }
            let Ok(value) = field.to_str() else {
                return malformed(MalformedReason::NotText);
            };
            Presented::Present(ApiKey(value.to_owned()))
        }

        KeyLocation::Query => query::value(parts, name).map(ApiKey),

        KeyLocation::Cookie => match crate::http::cookie::value_of(&parts.headers, name) {
            Ok(Some(value)) => Presented::Present(ApiKey(value.to_owned())),
            Ok(None) => Presented::Absent,
            Err(_) => malformed(MalformedReason::NotText),
        },
    }
}

/// The certificate chain the peer presented during the TLS handshake.
///
/// The one credential no request field carries.
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
/// [`Absent`](Presented::Absent) for every way of not having presented one: the
/// connection is not TLS, the listener does not verify client certificates,
/// the peer sent none, or this build has no `tls` feature and no embedding
/// recorded a chain through
/// [`Connection::from_tls_peer`](crate::extract::connection::Connection::from_tls_peer).
/// Never [`Malformed`](Presented::Malformed): the handshake already refused a
/// chain it could not read.
///
/// Not gated on `tls`: a service behind a TLS-terminating proxy sees no
/// certificates even with it on, so the deployment, not the feature, decides.
pub fn peer_certificates(parts: &Parts) -> Presented<PeerCertificates> {
    let Some(connection) = parts
        .extensions
        .get::<crate::extract::connection::Connection>()
    else {
        return Presented::Absent;
    };
    if connection.peer_certificates().is_empty() {
        return Presented::Absent;
    }
    Presented::Present(PeerCertificates(connection.clone()))
}

#[cfg(test)]
mod tests;
