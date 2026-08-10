//! The credentials a handler takes, and the scopes they must carry.
//!
//! Taking one of these as a handler argument enforces the requirement, adds the
//! scheme to the operation's `security`, and adds 401 and 403 to its
//! `responses`. There is no way to do one without the others — which is the
//! whole point.

use crate::{
    error::rejection::Rejection,
    extract::{FromRequestParts, describe::Describe},
    http::Parts,
    router::operation::OperationCx,
    security::{Authenticates, Authenticator, SecurityScheme},
};

/// A credential proving the request satisfies scheme `S`.
///
/// Taking this as a handler argument does three things at once: it enforces the
/// requirement, it adds `S` to the operation's `security`, and it adds 401 and
/// 403 to the operation's `responses`. There is no way to do one without the
/// others.
///
/// ```no_run
/// # use kynos::security::auth::Auth;
/// # struct Bearer; struct Claims;
/// async fn me(Auth(claims): Auth<Bearer>) {
///     todo!()
/// }
/// # impl kynos::security::SecurityScheme for Bearer {
/// #     const NAME: &'static str = "Bearer";
/// #     type Credential = Claims;
/// #     fn describe() -> kynos::openapi::SecurityScheme { todo!() }
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Auth<S: SecurityScheme>(pub S::Credential);

impl<S: SecurityScheme> Auth<S> {
    /// Unwraps the verified credential.
    pub fn into_inner(self) -> S::Credential {
        self.0
    }
}

impl<S: SecurityScheme> Describe for Auth<S> {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
        todo!()
    }
}

impl<C, S> FromRequestParts<C> for Auth<S>
where
    C: Authenticates<S> + Sync,
    S: SecurityScheme,
{
    type Rejection = Rejection;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        context
            .authenticator()
            .authenticate(parts, context)
            .await
            .map(Self)
    }
}

/// A named set of scopes.
///
/// Declared as a unit struct so that scope sets are types rather than string
/// literals repeated across handlers — a misspelled scope becomes a compile
/// error, and renaming one is a single edit.
pub trait Scopes: Send + Sync + 'static {
    /// The scopes required.
    const SCOPES: &'static [&'static str];
}

/// An [`Auth`] additionally requiring a set of scopes.
///
/// The scopes appear in the operation's security requirement, so a description
/// reader learns not just that a token is needed but which grants it must
/// carry.
///
/// A const generic would be the natural spelling, but `&'static [&'static str]`
/// is not a permitted const parameter type, so the scope set is a type
/// implementing [`Scopes`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Scoped<S: SecurityScheme, R: Scopes>(pub S::Credential, pub std::marker::PhantomData<R>);

impl<S: SecurityScheme, R: Scopes> Scoped<S, R> {
    /// Unwraps the verified and authorized credential.
    pub fn into_inner(self) -> S::Credential {
        self.0
    }
}

impl<S: SecurityScheme, R: Scopes> Describe for Scoped<S, R> {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
        todo!()
    }
}

impl<C, S, R> FromRequestParts<C> for Scoped<S, R>
where
    C: Authenticates<S> + Sync,
    S: SecurityScheme,
    R: Scopes,
{
    type Rejection = Rejection;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        let authenticator = context.authenticator();
        let credential = authenticator.authenticate(parts, context).await?;
        authenticator
            .authorize(&credential, R::SCOPES, context)
            .await?;
        Ok(Self(credential, std::marker::PhantomData))
    }
}
