//! What a carrier found where its scheme's credential travels: classified,
//! never refused.
//!
//! A carrier answers one of three things, and none of them is a refusal. The
//! guard answers an [`Absent`](Presented::Absent) credential itself, unless the
//! scheme hands absence on; a [`Malformed`](Presented::Malformed) one reaches
//! the [`Authenticator`](crate::security::Authenticator), which names why it
//! refuses it, or admits it, as the application decides. That holds for a
//! guard over one scheme; a combination asks only the schemes it gets to, as
//! [`Authenticator::authenticate`](crate::security::Authenticator::authenticate)
//! states.

use std::fmt;

use crate::error::rejection::AuthRejection;

/// What a [`Carries`](super::Carries) found where its scheme's credential
/// travels.
///
/// Three answers, and each means something different:
///
/// | Variant | The request | `Auth<S>` | `MaybeAuth<S>` |
/// | --- | --- | --- | --- |
/// | [`Absent`](Self::Absent) | presented nothing this scheme reads | 401 | anonymous |
/// | [`Malformed`](Self::Malformed) | presented something that is not a credential of this scheme | the authenticator decides | the authenticator decides |
/// | [`Present`](Self::Present) | presented a well-formed credential, not yet verified | the authenticator decides | the authenticator decides |
///
/// A malformed credential is not an anonymous request, so the guard never
/// treats it as one. A scheme whose [`OPTIONAL`](super::Carries::OPTIONAL) is
/// set hands absence to the authenticator too.
///
/// An authenticator that refuses whatever is not a well-formed credential
/// writes [`require`](Self::require):
///
/// ```
/// use kynos::security::carrier::{
///     BearerToken,
///     presented::{Malformed, MalformedReason, Presented},
/// };
///
/// let malformed: Presented<BearerToken> =
///     Presented::Malformed(Malformed::new(MalformedReason::OtherScheme));
/// let refused = malformed.require().expect_err("a malformed credential is refused");
/// assert_eq!(refused.status(), kynos::http::StatusCode::UNAUTHORIZED);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use = "a presented credential is checked, or refused, not dropped"]
pub enum Presented<T> {
    /// No credential of this scheme on the request.
    Absent,
    /// A field this scheme reads that does not hold a well-formed credential
    /// of it.
    Malformed(Malformed),
    /// A well-formed credential, not yet verified.
    Present(T),
}

impl<T> Presented<T> {
    /// The credential, or the refusal its absence or malformation earns.
    ///
    /// Absence is [`AuthRejection::unauthenticated`], and a malformed
    /// credential is [`Malformed::into_rejection`]'s: the same 401 the guard
    /// would have answered.
    ///
    /// # Errors
    ///
    /// When the credential is [`Absent`](Self::Absent) or
    /// [`Malformed`](Self::Malformed).
    pub fn require(self) -> Result<T, AuthRejection> {
        match self {
            Self::Absent => Err(AuthRejection::unauthenticated()),
            Self::Malformed(malformed) => Err(malformed.into_rejection()),
            Self::Present(credential) => Ok(credential),
        }
    }

    /// The credential where one was presented, `None` where none was, and the
    /// malformed credential's refusal otherwise.
    ///
    /// For a scheme that hands absence to the authenticator, which decides
    /// what an anonymous request may do.
    ///
    /// # Errors
    ///
    /// When the credential is [`Malformed`](Self::Malformed).
    pub fn into_result(self) -> Result<Option<T>, AuthRejection> {
        match self {
            Self::Absent => Ok(None),
            Self::Malformed(malformed) => Err(malformed.into_rejection()),
            Self::Present(credential) => Ok(Some(credential)),
        }
    }

    /// Maps a present credential with `read`, leaving absence and
    /// malformation as they are.
    ///
    /// For a hand-written carrier that wraps one of the readers in
    /// [`carrier`](super).
    pub fn map<U>(self, read: impl FnOnce(T) -> U) -> Presented<U> {
        match self {
            Self::Absent => Presented::Absent,
            Self::Malformed(malformed) => Presented::Malformed(malformed),
            Self::Present(credential) => Presented::Present(read(credential)),
        }
    }

    /// Classifies a present credential again with `read`, leaving absence and
    /// malformation as they are.
    ///
    /// For a carrier that reads more beside a credential, or reads it further,
    /// and finds it malformed where that fails.
    pub fn and_then<U>(self, read: impl FnOnce(T) -> Presented<U>) -> Presented<U> {
        match self {
            Self::Absent => Presented::Absent,
            Self::Malformed(malformed) => Presented::Malformed(malformed),
            Self::Present(credential) => read(credential),
        }
    }
}

/// A credential field that does not hold a well-formed credential of its
/// scheme, and why.
///
/// Holds no byte of what the request sent: a malformed credential may still be
/// a secret, mistyped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Malformed {
    /// The rule the field broke.
    reason: MalformedReason,
}

impl Malformed {
    /// A malformed credential, for `reason`.
    ///
    /// For a hand-written carrier; the readers in [`carrier`](super) build
    /// their own.
    #[must_use]
    pub const fn new(reason: MalformedReason) -> Self {
        Self { reason }
    }

    /// The rule the field broke.
    #[must_use]
    pub const fn reason(&self) -> MalformedReason {
        self.reason
    }

    /// The refusal a malformed credential earns by default:
    /// [`AuthRejection::unauthenticated`], a 401 the guard attaches the
    /// scheme's challenge to.
    #[must_use]
    pub fn into_rejection(self) -> AuthRejection {
        AuthRejection::unauthenticated()
    }
}

impl fmt::Display for Malformed {
    /// The reason's sentence.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.reason, formatter)
    }
}

impl std::error::Error for Malformed {}

/// The rule a [`Malformed`] credential broke.
///
/// For a log line or a metric label. The refusal on the wire is one 401
/// whatever the reason: telling a client which rule its credential broke helps
/// only the one probing for a credential that parses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MalformedReason {
    /// A credential field the request sent more than once: choosing either
    /// would let an appending proxy pick the credential.
    Repeated,
    /// A credential field whose value is not text: not visible ASCII for a
    /// header field or a cookie, not UTF-8 once a query value is decoded.
    NotText,
    /// An `Authorization` field with no scheme token before its credentials.
    NoScheme,
    /// An `Authorization` field holding another scheme's credentials.
    OtherScheme,
    /// Credentials outside their scheme's grammar: a bearer token that is no
    /// `b64token`, basic credentials that are not base64 of UTF-8 text with a
    /// colon in it.
    BadCredentials,
    /// An API key a hand-written carrier reads from `Authorization`, which
    /// belongs to the HTTP authentication schemes, whatever the request holds.
    ReservedField,
    /// A header the carrier reads beside a presented credential, under
    /// `#[security(..., reads = T)]`, that is missing or does not decode.
    AccompanyingHeaders,
}

impl MalformedReason {
    /// A stable `snake_case` name, for a metric label or a structured log.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Repeated => "repeated",
            Self::NotText => "not_text",
            Self::NoScheme => "no_scheme",
            Self::OtherScheme => "other_scheme",
            Self::BadCredentials => "bad_credentials",
            Self::ReservedField => "reserved_field",
            Self::AccompanyingHeaders => "accompanying_headers",
        }
    }
}

impl fmt::Display for MalformedReason {
    /// A sentence naming the rule, for a human reading a log.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Repeated => "the credential field was sent more than once",
            Self::NotText => "the credential field is not text",
            Self::NoScheme => "`Authorization` names no scheme",
            Self::OtherScheme => "`Authorization` holds another scheme's credentials",
            Self::BadCredentials => "the credentials are outside their scheme's grammar",
            Self::ReservedField => "an API key cannot travel in `Authorization`",
            Self::AccompanyingHeaders => "a header read beside the credential did not decode",
        })
    }
}

impl std::error::Error for MalformedReason {}

#[cfg(test)]
mod tests;
