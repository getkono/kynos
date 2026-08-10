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
pub mod schemes;

use std::future::Future;

use crate::{error::rejection::Rejection, http::Parts};

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
/// #     fn describe() -> kynos::openapi::SecurityScheme { todo!() }
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
}

/// Verifies a credential.
///
/// Kept separate from [`SecurityScheme`] because the two answer different
/// questions: the scheme says how a credential is *carried*, this says how it
/// is *checked*. Kynos deliberately does not ship a JWT verifier or a session
/// store — that is application policy, and prescribing it would be exactly the
/// kind of scope creep the project avoids.
pub trait Authenticator<S: SecurityScheme, C: Sync>: Send + Sync + 'static {
    /// Checks the credential carried by this request.
    ///
    /// Return [`Rejection::Unauthenticated`] when the credential is absent or
    /// invalid, and [`Rejection::Forbidden`] when it is valid but insufficient.
    fn authenticate(
        &self,
        parts: &Parts,
        context: &C,
    ) -> impl Future<Output = Result<S::Credential, Rejection>> + Send;

    /// Checks that an authenticated credential has every requested scope.
    fn authorize(
        &self,
        credential: &S::Credential,
        scopes: &'static [&'static str],
        context: &C,
    ) -> impl Future<Output = Result<(), Rejection>> + Send;
}

/// An application context that supplies an authenticator for scheme `S`.
///
/// This typed association replaces an erased authentication extension map: a
/// router using `Auth<S>` cannot be mounted with a context that does not prove
/// it can authenticate `S`.
pub trait Authenticates<S: SecurityScheme>: Sync + Sized {
    /// The concrete authenticator owned by this context.
    type Authenticator: Authenticator<S, Self>;

    /// Borrows the authenticator used for this scheme.
    fn authenticator(&self) -> &Self::Authenticator;
}
