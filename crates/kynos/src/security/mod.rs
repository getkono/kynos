//! Authentication and authorization, described by construction.
//!
//! The single rule here: **[`Auth`](auth::Auth) is the only way to guard an operation**.
//!
//! Requiring a credential and declaring it are the same act, so enforcement and
//! the description cannot drift apart.
//!
//! # One guard per operation
//!
//! A guard is a handler's first argument, and a handler takes at most one: two
//! would both be enforced while OpenAPI read their requirements as "either".
//! Schemes combine inside the guard's type parameter instead — see
//! [`requirement`].
//!
//! # How this module is laid out
//!
//! The traits live here; [`auth`] holds the guards a handler takes a verified
//! credential in, [`requirement`] the ways schemes combine inside one, and
//! [`schemes`] the schemes Kynos can describe without a derive.

pub mod auth;
pub mod carrier;
pub mod requirement;
pub mod schemes;

use std::future::Future;

use crate::{
    error::rejection::AuthRejection,
    extract::describe::Describe,
    http::Parts,
    response::{IntoResponse, Responses},
};

/// What closes the set of guards.
mod sealed {
    /// The private supertrait.
    pub trait Sealed {}
}

/// A handler argument that enforces a security requirement.
///
/// Sealed, and implemented by [`Auth`](auth::Auth),
/// [`MaybeAuth`](auth::MaybeAuth) and [`Scoped`](auth::Scoped). A guard is not
/// a [`FromRequestParts`](crate::extract::FromRequestParts): it has a slot of
/// its own, the first, the way a body has the last. A
/// [`Handler`](crate::handler::Handler) is implemented for functions with that
/// slot and for functions without it, and for nothing with two, so a second
/// guard is a compile error.
///
/// ```no_run
/// # use kynos::{
/// #     error::rejection::AuthRejection,
/// #     extract::params::path::Path,
/// #     security::{Authenticates, Authenticator, auth::Auth, carrier::BearerToken, schemes::Bearer},
/// # };
/// # struct Tokens;
/// # impl<C: Sync> Authenticator<Bearer, C> for Tokens {
/// #     async fn authenticate(&self, _: BearerToken, _: &C) -> Result<String, AuthRejection> {
/// #         Err(AuthRejection::unauthenticated())
/// #     }
/// #     async fn authorize(&self, _: &String, _: &[kynos::security::Unscoped], _: &C) -> Result<(), AuthRejection> {
/// #         Ok(())
/// #     }
/// # }
/// # struct App;
/// # impl Authenticates<Bearer> for App {
/// #     type Authenticator = Tokens;
/// #     fn authenticator(&self) -> &Tokens { &Tokens }
/// # }
/// # #[derive(kynos::Schema, kynos::PathParams)]
/// # struct Id { id: u64 }
/// # fn is_handler<C, A, H: kynos::handler::Handler<C, A>>(_: H) {}
/// async fn read(caller: Auth<Bearer>, Path(id): Path<Id>) {}
///
/// is_handler::<App, _, _>(read);
/// ```
///
/// The guard comes first:
///
/// ```compile_fail
/// # use kynos::{
/// #     error::rejection::AuthRejection,
/// #     extract::params::path::Path,
/// #     security::{Authenticates, Authenticator, auth::Auth, carrier::BearerToken, schemes::Bearer},
/// # };
/// # struct Tokens;
/// # impl<C: Sync> Authenticator<Bearer, C> for Tokens {
/// #     async fn authenticate(&self, _: BearerToken, _: &C) -> Result<String, AuthRejection> {
/// #         Err(AuthRejection::unauthenticated())
/// #     }
/// #     async fn authorize(&self, _: &String, _: &[kynos::security::Unscoped], _: &C) -> Result<(), AuthRejection> {
/// #         Ok(())
/// #     }
/// # }
/// # struct App;
/// # impl Authenticates<Bearer> for App {
/// #     type Authenticator = Tokens;
/// #     fn authenticator(&self) -> &Tokens { &Tokens }
/// # }
/// # #[derive(kynos::Schema, kynos::PathParams)]
/// # struct Id { id: u64 }
/// # fn is_handler<C, A, H: kynos::handler::Handler<C, A>>(_: H) {}
/// async fn read(Path(id): Path<Id>, caller: Auth<Bearer>) {}
///
/// is_handler::<App, _, _>(read);
/// ```
///
/// And comes once. Two schemes are one guard's
/// [`AnyOf`](requirement::AnyOf) or [`AllOf`](requirement::AllOf):
///
/// ```compile_fail
/// # use kynos::{
/// #     error::rejection::AuthRejection,
/// #     security::{Authenticates, Authenticator, auth::Auth, carrier::BearerToken, schemes::Bearer},
/// # };
/// # struct Tokens;
/// # impl<C: Sync> Authenticator<Bearer, C> for Tokens {
/// #     async fn authenticate(&self, _: BearerToken, _: &C) -> Result<String, AuthRejection> {
/// #         Err(AuthRejection::unauthenticated())
/// #     }
/// #     async fn authorize(&self, _: &String, _: &[kynos::security::Unscoped], _: &C) -> Result<(), AuthRejection> {
/// #         Ok(())
/// #     }
/// # }
/// # struct App;
/// # impl Authenticates<Bearer> for App {
/// #     type Authenticator = Tokens;
/// #     fn authenticator(&self) -> &Tokens { &Tokens }
/// # }
/// # fn is_handler<C, A, H: kynos::handler::Handler<C, A>>(_: H) {}
/// async fn read(caller: Auth<Bearer>, again: Auth<Bearer>) {}
///
/// is_handler::<App, _, _>(read);
/// ```
pub trait Guard<C>: sealed::Sealed + Describe + Sized + Send {
    /// How this guard refuses, and what that refusal looks like in the
    /// description.
    type Rejection: IntoResponse + Responses;

    /// Checks the request head, yielding the verified credential.
    ///
    /// Runs before every other argument is extracted, and reads the head
    /// without changing it.
    fn guard(
        parts: &Parts,
        context: &C,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send;
}

/// Compares two secrets without returning on the first byte that differs.
///
/// An ordinary `==` on a shared secret returns at the first difference, so its
/// timing says how much of a guess was right. Reach for this wherever an
/// [`Authenticator`] compares a credential against a value it holds -- an API
/// key against a table, a password against a stored one.
///
/// # What this does not promise
///
/// **The lengths are compared first, and a difference returns immediately.** A
/// secret's length is fixed by the scheme that issued it, so it is not secret.
///
/// **The guarantee is best-effort.** Every byte is folded into one accumulator
/// hidden behind [`black_box`](core::hint::black_box), which stops the loop
/// becoming an early return; a hard guarantee would need inline assembly, which
/// `unsafe_code = "forbid"` rules out.
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
/// #     type Scope = kynos::security::Unscoped;
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

    /// What one scope of this scheme is, as a value.
    ///
    /// Named once, here, so every [`Scopes`](auth::Scopes) set a
    /// [`Scoped`](auth::Scoped) argument pairs with this scheme is checked
    /// against it at compile time, and
    /// [`Authenticator::authorize`] receives the demanded scopes as this type
    /// rather than as strings to parse back. [`Unscoped`] where the scheme
    /// demands none.
    type Scope: Scope;

    /// The scheme's description.
    fn describe() -> kynos_openapi::SecurityScheme;

    /// The scopes this scheme requires by default.
    ///
    /// Declared on every requirement naming the scheme. OAuth 2.0 and OpenID
    /// Connect call them scopes; OpenAPI lets any other scheme list role names
    /// here instead.
    #[must_use]
    fn scopes() -> &'static [Self::Scope] {
        &[]
    }

    /// The `WWW-Authenticate` challenge sent with a 401, if this scheme has
    /// one.
    ///
    /// Declared on the scheme so the challenge in the description and the one
    /// on the wire are one string.
    #[must_use]
    fn challenge() -> Option<&'static str> {
        None
    }
}

/// One scope a [`SecurityScheme`] can demand, as a value.
///
/// A scheme names its scope type once, as
/// [`SecurityScheme::Scope`], and a [`Scoped`](auth::Scoped) argument pairing
/// it with a [`Scopes`](auth::Scopes) set of another type does not compile. A
/// misspelt scope is an unknown variant rather than a string no authorizer
/// recognises; [`name`](Scope::name) is the one place it becomes a string,
/// for the description and for comparing against what a token carries.
///
/// ```
/// use kynos::security::Scope;
///
/// #[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// enum Grant {
///     ReadReports,
///     Admin,
/// }
///
/// impl Scope for Grant {
///     fn name(&self) -> &'static str {
///         match self {
///             Self::ReadReports => "reports:read",
///             Self::Admin => "admin",
///         }
///     }
/// }
///
/// assert_eq!(Grant::ReadReports.name(), "reports:read");
/// ```
pub trait Scope: Send + Sync + 'static {
    /// The scope as a description and a token spell it.
    fn name(&self) -> &'static str;
}

/// The [`Scope`] of a scheme that demands none.
///
/// Uninhabited, so a scope set over it is empty and a scheme declaring it has
/// no scope to name. The scope type of [`schemes`]' built-ins by default, and
/// of a derived scheme that names no `scope = Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Unscoped {}

impl Scope for Unscoped {
    // Uninhabited, so no test can call it and every mutant survives.
    #[cfg_attr(test, mutants::skip)]
    fn name(&self) -> &'static str {
        match *self {}
    }
}

/// Verifies a credential.
///
/// The scheme says how a credential is *carried*; this says how it is
/// *checked*. Kynos ships no JWT verifier or session store: that is
/// application policy.
///
/// [`authenticate`](Authenticator::authenticate) receives the credential the
/// scheme's own carrier already extracted, not a `&Parts`, so an authenticator
/// cannot read a field the scheme did not declare.
pub trait Authenticator<S: carrier::Carries, C: Sync>: Send + Sync + 'static {
    /// Checks the credential this request presented.
    ///
    /// Return [`AuthRejection::unauthenticated`] when the credential is invalid
    /// and [`AuthRejection::forbidden`] when it is valid but the application
    /// refuses it anyway — a suspended account is a *valid* credential the
    /// application declines, which is a 403 and not a 401. Such a refusal is
    /// the application's own, so [`AuthRejection::forbidden_as`] names it where
    /// there is a problem type for it, exactly as in
    /// [`authorize`](Authenticator::authorize). The challenge is left unset:
    /// [`Auth`](auth::Auth) attaches
    /// [`challenge`](SecurityScheme::challenge) on the way out, so the wire and
    /// the description cannot name different ones.
    ///
    /// A credential that was *absent* never reaches here — that is
    /// [`Auth`](auth::Auth)'s 401 and [`MaybeAuth`](auth::MaybeAuth)'s
    /// anonymity — unless the scheme was derived with `carrier = optional`.
    /// Its `Presented` is then an `Option`, and `None` is this method's to
    /// admit, for a deployment configured with no credentials, or to refuse.
    fn authenticate(
        &self,
        presented: S::Presented,
        context: &C,
    ) -> impl Future<Output = Result<S::Credential, AuthRejection>> + Send;

    /// Checks that an authenticated credential has every requested scope.
    ///
    /// `scopes` are the scheme's own [`Scope`](SecurityScheme::Scope) values,
    /// so a scope this method does not handle is a non-exhaustive `match`
    /// rather than a string it silently skips.
    ///
    /// A refusal is [`AuthRejection::forbidden`], or
    /// [`AuthRejection::forbidden_as`] where the application has a problem type
    /// for the rule that refused.
    fn authorize(
        &self,
        credential: &S::Credential,
        scopes: &[S::Scope],
        context: &C,
    ) -> impl Future<Output = Result<(), AuthRejection>> + Send;
}

/// An application context that supplies an authenticator for scheme `S`.
///
/// A router using `Auth<S>` cannot be mounted with a context that does not
/// prove it can authenticate `S`.
pub trait Authenticates<S: carrier::Carries>: Sync + Sized {
    /// The concrete authenticator owned by this context.
    type Authenticator: Authenticator<S, Self>;

    /// Borrows the authenticator used for this scheme.
    fn authenticator(&self) -> &Self::Authenticator;
}

#[cfg(test)]
mod tests;
