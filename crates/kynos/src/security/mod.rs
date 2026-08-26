//! Authentication and authorization, described by construction.
//!
//! The single rule here: **[`Auth`](auth::Auth) is the only way to guard an operation**.
//!
//! That is what stops enforcement and documentation from drifting apart. In
//! utoipa or aide, the `security` block is written by hand next to the code
//! that checks the credential, and nothing keeps the two in step — an endpoint
//! can be guarded and undocumented, or documented and unguarded, and the
//! description looks equally plausible either way. Here, requiring a credential
//! and declaring it are the same act.
//!
//! # How this module is laid out
//!
//! The three traits live here; [`auth`] holds the extractors a handler takes a
//! verified credential in, and [`schemes`] the schemes Kynos can describe
//! without a derive.

pub mod auth;
pub mod carrier;
pub mod schemes;

use std::future::Future;

use crate::error::rejection::AuthRejection;

/// Compares two secrets without returning on the first byte that differs.
///
/// An ordinary `==` on a shared secret returns as soon as it finds a
/// difference, so how long it took says how much of the secret was right. That
/// turns guessing a key into guessing it one byte at a time. Reach for this
/// wherever an [`Authenticator`] compares a credential against a value it holds
/// -- an API key against a table, a password against a stored one.
///
/// # What this does not promise
///
/// **The lengths are compared first, and a difference returns immediately.** A
/// secret's length is not secret in any of the cases here: it is fixed by the
/// scheme that issued it, and padding to hide it would compare a secret against
/// something that is not one.
///
/// **The guarantee is best-effort.** A hard one needs a barrier the compiler
/// cannot see through, and `unsafe_code = "forbid"` puts inline assembly out of
/// reach. What is here folds every byte into one accumulator and hides the
/// result behind [`black_box`](core::hint::black_box), which is what stops the
/// loop being rewritten into an early return. That is the strongest statement
/// safe Rust supports, and it is stated rather than implied because the
/// difference matters to anyone deciding whether it is enough.
///
/// ```
/// use kynos::security::constant_time_eq;
///
/// assert!(constant_time_eq(b"a-shared-secret", b"a-shared-secret"));
/// assert!(!constant_time_eq(b"a-shared-secret", b"a-shared-secre!"));
/// assert!(!constant_time_eq(b"short", b"longer"));
/// ```
#[must_use]
pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }

    let difference = left
        .iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right));

    core::hint::black_box(difference) == 0
}

/// A security scheme, as a type.
///
/// Derived with `#[derive(SecurityScheme)]` on a unit struct:
///
/// ```no_run
/// # use kynos::security::SecurityScheme;
/// # struct Bearer;
/// # impl SecurityScheme for Bearer {
/// #     const NAME: &'static str = "Bearer";
/// #     type Credential = String;
/// #     fn describe() -> kynos::openapi::SecurityScheme {
/// #         kynos::openapi::SecurityScheme::bearer(None)
/// #     }
/// # }
/// ```
///
/// The scheme registers itself under [`NAME`](SecurityScheme::NAME) in
/// `components.securitySchemes` the first time an [`Auth`](auth::Auth) referencing it is
/// described.
pub trait SecurityScheme: Send + Sync + 'static {
    /// The component name this scheme is registered under.
    const NAME: &'static str;

    /// What a successful authentication yields to the handler.
    type Credential: Send;

    /// The scheme's description.
    fn describe() -> kynos_openapi::SecurityScheme;

    /// The scopes this scheme requires by default.
    ///
    /// Meaningful only for OAuth 2.0 and OpenID Connect.
    fn scopes() -> &'static [&'static str] {
        &[]
    }

    /// The `WWW-Authenticate` challenge sent with a 401, if this scheme has
    /// one.
    ///
    /// Declared here rather than in the authenticator so that the challenge in
    /// the description and the challenge on the wire are one string. A client
    /// has to handle it, which makes it part of what the 401 response *is*
    /// rather than an implementation detail of enforcing the scheme.
    fn challenge() -> Option<&'static str> {
        None
    }
}

/// Verifies a credential.
///
/// Kept separate from [`SecurityScheme`] because the two answer different
/// questions: the scheme says how a credential is *carried*, this says how it
/// is *checked*. Kynos deliberately does not ship a JWT verifier or a session
/// store — that is application policy, and prescribing it would be exactly the
/// kind of scope creep the project avoids.
///
/// # Why this is not handed the request
///
/// [`authenticate`](Authenticator::authenticate) receives the credential the
/// scheme's own carrier already extracted, not a `&Parts`. That is what makes
/// the field a verifier reads and the field the description advertises one
/// string: an authenticator *cannot* reach for a header the scheme did not
/// declare, because it is never given anywhere to reach.
///
/// Every framework that configures a credential "finder" beside its
/// documentation has two statements that agree until someone edits one. There
/// is one here.
pub trait Authenticator<S: carrier::Carries, C: Sync>: Send + Sync + 'static {
    /// Checks the credential this request presented.
    ///
    /// Return [`AuthRejection::unauthenticated`] when the credential is invalid
    /// and [`AuthRejection::Forbidden`] when it is valid but insufficient. The
    /// challenge is left unset: [`Auth`](auth::Auth) attaches
    /// [`challenge`](SecurityScheme::challenge) on the way out, so the wire and
    /// the description cannot name different ones.
    ///
    /// A credential that was *absent* never reaches here — that is
    /// [`Auth`](auth::Auth)'s 401 and [`MaybeAuth`](auth::MaybeAuth)'s
    /// anonymity, and neither is a question a verifier can answer.
    fn authenticate(
        &self,
        presented: S::Presented,
        context: &C,
    ) -> impl Future<Output = Result<S::Credential, AuthRejection>> + Send;

    /// Checks that an authenticated credential has every requested scope.
    fn authorize(
        &self,
        credential: &S::Credential,
        scopes: &'static [&'static str],
        context: &C,
    ) -> impl Future<Output = Result<(), AuthRejection>> + Send;
}

/// An application context that supplies an authenticator for scheme `S`.
///
/// This typed association replaces an erased authentication extension map: a
/// router using `Auth<S>` cannot be mounted with a context that does not prove
/// it can authenticate `S`.
pub trait Authenticates<S: carrier::Carries>: Sync + Sized {
    /// The concrete authenticator owned by this context.
    type Authenticator: Authenticator<S, Self>;

    /// Borrows the authenticator used for this scheme.
    fn authenticator(&self) -> &Self::Authenticator;
}

#[cfg(test)]
mod tests {
    use super::constant_time_eq;

    /// Agreement with `==`, swept over every way two short strings can differ.
    ///
    /// The property is that this is `==` and nothing else: a comparison that
    /// were merely *slow* would satisfy any single case. What cannot be
    /// asserted here is the timing itself — a wall-clock assertion is a flake
    /// on a shared runner, and `docs/testing.md` would rather have no test than
    /// a retried one — so the constant-time half is documented as best-effort
    /// on the item instead.
    #[test]
    fn the_comparison_agrees_with_equality_everywhere() {
        let inputs: &[&[u8]] = &[
            b"",
            b"a",
            b"b",
            b"ab",
            b"ba",
            b"aa",
            b"abc",
            b"abd",
            b"dbc",
            b"abcd",
            &[0x00],
            &[0xff],
            &[0x00, 0x00],
        ];

        for left in inputs {
            for right in inputs {
                assert_eq!(
                    constant_time_eq(left, right),
                    left == right,
                    "comparing {left:?} against {right:?}"
                );
            }
        }
    }

    /// A difference in the last byte is found as surely as one in the first.
    ///
    /// The failure this rules out is a fold that stops early: with `&&` in
    /// place of `|`, a secret differing only at the end would compare equal for
    /// every prefix that matched.
    #[test]
    fn a_difference_anywhere_is_a_difference() {
        let secret = b"0123456789abcdef";

        for index in 0..secret.len() {
            let mut guess = *secret;
            guess[index] ^= 0x01;
            assert!(
                !constant_time_eq(secret, &guess),
                "a difference at byte {index} went unnoticed"
            );
        }

        assert!(constant_time_eq(secret, secret));
    }
}
