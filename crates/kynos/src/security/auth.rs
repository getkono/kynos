//! The credentials a handler takes, and the scopes they must carry.
//!
//! Taking one of these as a handler argument enforces the requirement, adds the
//! scheme to the operation's `security`, and adds 401 and 403 to its
//! `responses`. There is no way to do one without the others — which is the
//! whole point.

use kynos_openapi::{
    ComponentName, Header, Schema, SecurityRequirement, StatusPattern,
    model::schema::types::SchemaType,
};

use crate::{
    error::rejection::AuthRejection,
    extract::{FromRequestParts, describe::Describe},
    http::{HeaderValue, Parts, StatusCode},
    response::Responses,
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
/// #     fn describe() -> kynos::openapi::SecurityScheme {
/// #         kynos::openapi::SecurityScheme::bearer(None)
/// #     }
/// # }
/// ```
pub struct Auth<S: SecurityScheme>(pub S::Credential);

// Hand-written rather than derived: a derive bounds the implementation on the
// *scheme*, which is a marker and carries nothing, while what is actually
// being cloned or compared is the credential. `Default` and `Ord` are absent
// on purpose — `Auth::default()` would be an unverified credential, and there
// is no meaningful order on one.
impl<S: SecurityScheme> Clone for Auth<S>
where
    S::Credential: Clone,
{
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<S: SecurityScheme> Copy for Auth<S> where S::Credential: Copy {}

impl<S: SecurityScheme> std::fmt::Debug for Auth<S>
where
    S::Credential: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Auth").field(&self.0).finish()
    }
}

impl<S: SecurityScheme> PartialEq for Auth<S>
where
    S::Credential: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<S: SecurityScheme> Eq for Auth<S> where S::Credential: Eq {}

impl<S: SecurityScheme> Auth<S> {
    /// Unwraps the verified credential.
    pub fn into_inner(self) -> S::Credential {
        self.0
    }
}

impl<S: SecurityScheme> Describe for Auth<S> {
    fn describe(operation: &mut OperationCx<'_>) {
        declare::<S>(operation, S::scopes().to_vec());
    }
}

impl<C, S> FromRequestParts<C> for Auth<S>
where
    C: Authenticates<S> + Sync,
    S: SecurityScheme,
{
    type Rejection = AuthRejection;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        context
            .authenticator()
            .authenticate(parts, context)
            .await
            .map(Self)
            // The challenge is the scheme's, not the authenticator's, and it is
            // attached here so that it is the same string `describe` declared.
            .map_err(|rejection| rejection.with_challenge(S::challenge()))
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
pub struct Scoped<S: SecurityScheme, R: Scopes>(pub S::Credential, pub std::marker::PhantomData<R>);

// See `Auth`: bounded on the credential, and without `Default`.
impl<S: SecurityScheme, R: Scopes> Clone for Scoped<S, R>
where
    S::Credential: Clone,
{
    fn clone(&self) -> Self {
        Self(self.0.clone(), std::marker::PhantomData)
    }
}

impl<S: SecurityScheme, R: Scopes> Copy for Scoped<S, R> where S::Credential: Copy {}

impl<S: SecurityScheme, R: Scopes> std::fmt::Debug for Scoped<S, R>
where
    S::Credential: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Scoped").field(&self.0).finish()
    }
}

impl<S: SecurityScheme, R: Scopes> PartialEq for Scoped<S, R>
where
    S::Credential: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<S: SecurityScheme, R: Scopes> Eq for Scoped<S, R> where S::Credential: Eq {}

impl<S: SecurityScheme, R: Scopes> Scoped<S, R> {
    /// Unwraps the verified and authorized credential.
    pub fn into_inner(self) -> S::Credential {
        self.0
    }
}

impl<S: SecurityScheme, R: Scopes> Describe for Scoped<S, R> {
    fn describe(operation: &mut OperationCx<'_>) {
        // "An `Auth` additionally requiring a set of scopes": the scheme's own
        // defaults still apply, and `R`'s are added to them rather than
        // replacing them. Declaring a scope twice would name it twice in the
        // requirement, so the union is taken by hand.
        let mut scopes = S::scopes().to_vec();
        for scope in R::SCOPES {
            if !scopes.contains(scope) {
                scopes.push(scope);
            }
        }
        declare::<S>(operation, scopes);
    }
}

impl<C, S, R> FromRequestParts<C> for Scoped<S, R>
where
    C: Authenticates<S> + Sync,
    S: SecurityScheme,
    R: Scopes,
{
    type Rejection = AuthRejection;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        // Both halves, because a `authorize` that answers 401 rather than 403
        // owes the client a challenge for the same reason `authenticate` does.
        let challenged = |rejection: AuthRejection| rejection.with_challenge(S::challenge());

        let authenticator = context.authenticator();
        let credential = authenticator
            .authenticate(parts, context)
            .await
            .map_err(challenged)?;
        authenticator
            .authorize(&credential, R::SCOPES, context)
            .await
            .map_err(challenged)?;
        Ok(Self(credential, std::marker::PhantomData))
    }
}

/// Declares scheme `S` as required by this operation, and defines it.
///
/// The three halves are one act: the requirement names the scheme, the
/// registration defines it under the same key, and the 401 carries the
/// challenge the scheme itself supplies.
fn declare<S: SecurityScheme>(operation: &mut OperationCx<'_>, scopes: Vec<&'static str>) {
    // Before the header, not after: `add_response_header` invents a thinly
    // described 401 when the operation declares none, and merging cannot
    // replace a response that already exists.
    let responses = AuthRejection::responses(operation.registry());
    operation.add_responses(responses);

    // RFC 9110 section 11.6.1: a 401 MUST carry at least one challenge. Only a
    // scheme that has one declares it, so a credential carried outside the
    // `Authorization` header -- an API key, a cookie, a client certificate --
    // advertises nothing a client could not answer.
    //
    // The `HeaderValue` round trip is the same test `AuthRejection` applies
    // before writing the header, so a challenge that cannot be a field value is
    // absent from both the response and the description rather than one of
    // them.
    if let Some(challenge) = S::challenge().filter(|value| HeaderValue::from_str(value).is_ok()) {
        operation.add_response_header(
            StatusPattern::Code(StatusCode::UNAUTHORIZED.as_u16()),
            "WWW-Authenticate",
            Header::new(Schema::of_type(SchemaType::String))
                .required(true)
                .with_description(
                    "The challenge the client must answer, per RFC 9110 section 11.6.1.",
                )
                .with_example(challenge),
        );
    }

    // One name for both halves, so the scheme the requirement demands and the
    // scheme the document defines cannot be different keys.
    let name = component_name::<S>();
    operation.add_security(SecurityRequirement::scoped(name.as_str(), scopes));
    operation.add_security_scheme(name, S::describe());
}

/// The component key scheme `S` is both registered and required under.
///
/// [`SecurityScheme::NAME`] is an ordinary `&'static str`, so it need not be a
/// legal component key, and [`Describe`] has no way to report that it was not.
/// Sanitizing rather than refusing is what keeps the requirement and the
/// registration naming one string, which is the disagreement
/// [`OperationCx::add_security_scheme`] exists to prevent. Only an empty name
/// fails to sanitize, and the scheme's own type name stands in for it.
fn component_name<S: SecurityScheme>() -> ComponentName {
    ComponentName::sanitized(S::NAME).unwrap_or_else(|_| {
        ComponentName::sanitized(std::any::type_name::<S>())
            .expect("a Rust type name is never empty")
    })
}
