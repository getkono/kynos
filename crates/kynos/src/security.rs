//! Authentication and authorization, described by construction.
//!
//! The single rule here: **[`Auth`] is the only way to guard an operation**.
//!
//! That is what stops enforcement and documentation from drifting apart. In
//! utoipa or aide, the `security` block is written by hand next to the code
//! that checks the credential, and nothing keeps the two in step — an endpoint
//! can be guarded and undocumented, or documented and unguarded, and the
//! description looks equally plausible either way. Here, requiring a credential
//! and declaring it are the same act.

use std::future::Future;

use crate::{
    error::Rejection,
    extract::{FromRequestParts, describe::Describe},
    http::Parts,
    router::OperationCx,
};

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
/// `components.securitySchemes` the first time an [`Auth`] referencing it is
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

/// A credential proving the request satisfies scheme `S`.
///
/// Taking this as a handler argument does three things at once: it enforces the
/// requirement, it adds `S` to the operation's `security`, and it adds 401 and
/// 403 to the operation's `responses`. There is no way to do one without the
/// others.
///
/// ```no_run
/// # use kynos::security::Auth;
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

/// The schemes Kynos knows how to describe.
///
/// Each is a unit struct implementing [`SecurityScheme`]; `#[derive(SecurityScheme)]`
/// exists for the cases these do not cover, such as an API key under a
/// non-standard header name.
pub mod schemes {
    use super::SecurityScheme;

    /// HTTP bearer authentication, per RFC 6750.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Bearer;

    /// HTTP basic authentication, per RFC 7617.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Basic;

    /// An API key carried in a header, query parameter or cookie.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct ApiKey;

    /// OAuth 2.0.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct OAuth2;

    /// OpenID Connect Discovery.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct OpenIdConnect;

    /// Mutual TLS client certificate authentication.
    ///
    /// Declared automatically when the listener is configured to verify client
    /// certificates, so turning on mTLS cannot leave the description silent
    /// about it.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct MutualTls;

    impl SecurityScheme for Bearer {
        const NAME: &'static str = "Bearer";
        type Credential = String;

        fn describe() -> kynos_openapi::SecurityScheme {
            todo!()
        }
    }

    impl SecurityScheme for Basic {
        const NAME: &'static str = "Basic";
        type Credential = (String, String);

        fn describe() -> kynos_openapi::SecurityScheme {
            todo!()
        }
    }

    impl SecurityScheme for ApiKey {
        const NAME: &'static str = "ApiKey";
        type Credential = String;

        fn describe() -> kynos_openapi::SecurityScheme {
            todo!()
        }
    }

    impl SecurityScheme for OAuth2 {
        const NAME: &'static str = "OAuth2";
        type Credential = String;

        fn describe() -> kynos_openapi::SecurityScheme {
            todo!()
        }
    }

    impl SecurityScheme for OpenIdConnect {
        const NAME: &'static str = "OpenIdConnect";
        type Credential = String;

        fn describe() -> kynos_openapi::SecurityScheme {
            todo!()
        }
    }

    impl SecurityScheme for MutualTls {
        const NAME: &'static str = "MutualTls";
        type Credential = Vec<u8>;

        fn describe() -> kynos_openapi::SecurityScheme {
            todo!()
        }
    }
}
