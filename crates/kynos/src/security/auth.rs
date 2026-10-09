//! The guards a handler takes, and the scopes they must carry.
//!
//! Taking one of these as a handler's [`Guard`] enforces the requirement, sets
//! the operation's `security`, and adds 401 and 403 to its `responses`, as one
//! act.

use kynos_openapi::{
    Header, Schema, SecurityRequirement, StatusPattern, model::schema::types::SchemaType,
};

use crate::{
    error::rejection::{AuthRejection, ScopedRejection, auth_responses},
    extract::describe::Describe,
    http::{HeaderValue, Parts, StatusCode},
    router::operation::OperationCx,
    security::{
        Authenticates, Authenticator, Guard, SecurityScheme,
        carrier::Carries,
        requirement::{CheckedBy, Requirement, require},
        sealed,
    },
};

/// A credential proving the request satisfies requirement `S`.
///
/// Taking this as a handler's guard — its first argument — does three things at
/// once: it enforces the requirement, it sets the operation's `security` to
/// `S`'s, and it adds 401 and 403 to the operation's `responses`. There is no
/// way to do one without the others.
///
/// `S` is a scheme, or schemes combined through
/// [`AnyOf`](crate::security::requirement::AnyOf) or
/// [`AllOf`](crate::security::requirement::AllOf), which is the only way an
/// operation demands more than one: a handler takes one guard.
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
pub struct Auth<S: Requirement>(pub S::Credential);

// Hand-written so the bounds fall on the credential, not the marker scheme. No
// `Default` (an unverified credential) and no `Ord`.
impl<S: Requirement> Clone for Auth<S>
where
    S::Credential: Clone,
{
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<S: Requirement> Copy for Auth<S> where S::Credential: Copy {}

impl<S: Requirement> std::fmt::Debug for Auth<S>
where
    S::Credential: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Auth").field(&self.0).finish()
    }
}

impl<S: Requirement> PartialEq for Auth<S>
where
    S::Credential: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<S: Requirement> Eq for Auth<S> where S::Credential: Eq {}

impl<S: Requirement> Auth<S> {
    /// Unwraps the verified credential.
    pub fn into_inner(self) -> S::Credential {
        self.0
    }
}

impl<S: Requirement> Describe for Auth<S> {
    /// Declares `about:blank` for its 403, and cannot declare anything else.
    ///
    /// The type on a named 403 is [`Scopes::FORBIDDEN_TYPE`], and this argument
    /// names no scope set; [`Scoped`] is the guard that does.
    fn describe(operation: &mut OperationCx<'_>) {
        let security = S::declare(operation);
        declare(operation, S::challenge(), security, None);
    }
}

impl<S: Requirement> sealed::Sealed for Auth<S> {}

impl<C, S> Guard<C> for Auth<S>
where
    C: Sync,
    S: CheckedBy<C>,
{
    type Rejection = AuthRejection;

    async fn guard(parts: &Parts, context: &C) -> Result<Self, Self::Rejection> {
        // The challenge is attached here so it is the string `describe`
        // declared; an absent credential is `Auth`'s own 401.
        S::check(parts, context)
            .await
            .and_then(|checked| checked.ok_or_else(AuthRejection::unauthenticated))
            .map(Self)
            .map_err(|rejection| rejection.with_challenge(S::challenge()))
    }
}

/// A credential proving requirement `S`, when the request presented one.
///
/// Declares `security: [{}, {S: []}]` — the empty requirement first, which is
/// how OpenAPI spells "anonymous access is also permitted". For a combined `S`
/// the empty requirement leads `S`'s own alternatives, so
/// `MaybeAuth<AnyOf<(A, B)>>` declares `[{}, {A: []}, {B: []}]`.
///
/// A credential that is present and wrong is still a 401. Only *absence* is
/// anonymity.
///
/// ```no_run
/// # use kynos::security::auth::MaybeAuth;
/// # struct Bearer; struct Claims;
/// async fn feed(MaybeAuth(caller): MaybeAuth<Bearer>) {
///     match caller {
///         Some(claims) => todo!("the personalised feed"),
///         None => todo!("the public one"),
///     }
/// }
/// # impl kynos::security::SecurityScheme for Bearer {
/// #     const NAME: &'static str = "Bearer";
/// #     type Credential = Claims;
/// #     fn describe() -> kynos::openapi::SecurityScheme {
/// #         kynos::openapi::SecurityScheme::bearer(None)
/// #     }
/// # }
/// ```
pub struct MaybeAuth<S: Requirement>(pub Option<S::Credential>);

// As on `Auth`: bounded on the credential, without `Default` or `Ord`.
impl<S: Requirement> Clone for MaybeAuth<S>
where
    S::Credential: Clone,
{
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<S: Requirement> Copy for MaybeAuth<S> where S::Credential: Copy {}

impl<S: Requirement> std::fmt::Debug for MaybeAuth<S>
where
    S::Credential: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("MaybeAuth").field(&self.0).finish()
    }
}

impl<S: Requirement> PartialEq for MaybeAuth<S>
where
    S::Credential: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<S: Requirement> Eq for MaybeAuth<S> where S::Credential: Eq {}

impl<S: Requirement> MaybeAuth<S> {
    /// Unwraps the verified credential, if the request carried one.
    pub fn into_inner(self) -> Option<S::Credential> {
        self.0
    }
}

impl<S: Requirement> Describe for MaybeAuth<S> {
    /// Declares `about:blank` for its 403: naming no scope set, it has no
    /// [`Scopes::FORBIDDEN_TYPE`] to declare.
    fn describe(operation: &mut OperationCx<'_>) {
        let mut security = vec![SecurityRequirement::anonymous()];
        security.extend(S::declare(operation));
        declare(operation, S::challenge(), security, None);
    }
}

impl<S: Requirement> sealed::Sealed for MaybeAuth<S> {}

impl<C, S> Guard<C> for MaybeAuth<S>
where
    C: Sync,
    S: CheckedBy<C>,
{
    type Rejection = AuthRejection;

    async fn guard(parts: &Parts, context: &C) -> Result<Self, Self::Rejection> {
        // Absent is anonymity, malformed is a 401, and present is a check.
        S::check(parts, context)
            .await
            .map(Self)
            .map_err(|rejection| rejection.with_challenge(S::challenge()))
    }
}

/// A named set of scopes.
///
/// Declared as a unit struct, so a scope set is named once rather than
/// repeated as string literals across handlers.
///
/// ```
/// use kynos::security::auth::Scopes;
///
/// /// What an administrative endpoint demands, and what refusing it is called.
/// struct Admin;
///
/// impl Scopes for Admin {
///     const SCOPES: &'static [&'static str] = &["admin"];
///     const FORBIDDEN_TYPE: Option<&'static str> =
///         Some("https://errors.example.com/insufficient-scope");
/// }
///
/// /// A scope set naming no type, which is every one written before this const
/// /// existed and still the ordinary case.
/// struct ReadReports;
///
/// impl Scopes for ReadReports {
///     const SCOPES: &'static [&'static str] = &["reports:read"];
/// }
///
/// assert_eq!(ReadReports::FORBIDDEN_TYPE, None);
/// ```
pub trait Scopes: Send + Sync + 'static {
    /// The scopes required.
    const SCOPES: &'static [&'static str];

    /// The problem `type` a refusal of *these* scopes may publish, beside
    /// `about:blank`.
    ///
    /// The description half of
    /// [`AuthRejection::forbidden_as`](crate::error::rejection::AuthRejection::forbidden_as):
    /// says once which refusal a [`Scoped<S, R>`](Scoped) argument can name.
    ///
    /// # What it declares
    ///
    /// A `Some` narrows the operation's 403 to a choice between `about:blank`
    /// and this URI — **both**, never this one alone, since
    /// [`AuthRejection::forbidden()`](crate::error::rejection::AuthRejection::forbidden)
    /// stays available to every authorizer.
    ///
    /// A `None`, the default, declares the shared `Problem` component and
    /// narrows nothing.
    ///
    /// # What it promises
    ///
    /// That every 403 a `Scoped<S, R>` argument produces carries this URI or
    /// `about:blank` — the whole guard, so an [`Authenticator::authenticate`]
    /// refusing with a *third* URI breaks it too. The type system does not hold
    /// it; the conformance harness checks it.
    ///
    /// [`Auth<S>`](Auth) and [`MaybeAuth<S>`](MaybeAuth) name no scope set and
    /// have no counterpart.
    const FORBIDDEN_TYPE: Option<&'static str> = None;
}

/// An [`Auth`] additionally requiring a set of scopes.
///
/// The scopes appear in the operation's security requirement, so a description
/// reader learns not just that a token is needed but which grants it must
/// carry.
///
/// The scope set is a type implementing [`Scopes`], since
/// `&'static [&'static str]` is not a permitted const parameter type. The
/// marker is private, so a handler destructures `Scoped(claims)` the way it
/// destructures [`Auth`] and [`MaybeAuth`].
pub struct Scoped<S: SecurityScheme, R: Scopes>(pub S::Credential, std::marker::PhantomData<R>);

// As on `Auth`: bounded on the credential, without `Default`.
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
    /// Wraps a credential that has been verified and authorized.
    ///
    /// The marker field is private, so this is how one is built from outside
    /// the crate.
    pub fn new(credential: S::Credential) -> Self {
        Self(credential, std::marker::PhantomData)
    }

    /// Unwraps the verified and authorized credential.
    #[must_use]
    pub fn into_inner(self) -> S::Credential {
        self.0
    }
}

impl<S: SecurityScheme, R: Scopes> Describe for Scoped<S, R> {
    fn describe(operation: &mut OperationCx<'_>) {
        // `R`'s scopes join the scheme's defaults rather than replacing them,
        // without naming one twice.
        let mut scopes = S::scopes().to_vec();
        for scope in R::SCOPES {
            if !scopes.contains(scope) {
                scopes.push(scope);
            }
        }
        let security = vec![require::<S>(operation, scopes)];
        declare(operation, S::challenge(), security, R::FORBIDDEN_TYPE);
    }
}

impl<S: SecurityScheme, R: Scopes> sealed::Sealed for Scoped<S, R> {}

impl<C, S, R> Guard<C> for Scoped<S, R>
where
    C: Authenticates<S>,
    S: Carries,
    R: Scopes,
{
    /// [`AuthRejection`] described against `R`, which is what narrows the 403
    /// this argument declares.
    type Rejection = ScopedRejection<R>;

    async fn guard(parts: &Parts, context: &C) -> Result<Self, Self::Rejection> {
        // On both halves: an `authorize` answering 401 owes a challenge too.
        let challenged = |rejection: AuthRejection| {
            ScopedRejection::new(rejection.with_challenge(S::challenge()))
        };

        let presented = S::present(parts)
            .map_err(challenged)?
            .ok_or_else(AuthRejection::unauthenticated)
            .map_err(challenged)?;

        let authenticator = context.authenticator();
        let credential = authenticator
            .authenticate(presented, context)
            .await
            .map_err(challenged)?;
        authenticator
            .authorize(&credential, R::SCOPES, context)
            .await
            .map_err(challenged)?;
        Ok(Self(credential, std::marker::PhantomData))
    }
}

/// Sets the operation's `security` to the guard's, and declares how it refuses.
///
/// `security` is the guard's whole list, set once rather than appended to.
/// `forbidden_type` is [`Scopes::FORBIDDEN_TYPE`] where the argument named a
/// scope set, and `None` where it named only a scheme.
fn declare(
    operation: &mut OperationCx<'_>,
    challenge: Option<&'static str>,
    security: Vec<SecurityRequirement>,
    forbidden_type: Option<&'static str>,
) {
    // Before the header: `add_response_header` invents a thin 401 when none is
    // declared, and merging cannot replace an existing response.
    let responses = auth_responses(operation.registry(), forbidden_type);
    operation.add_responses(&responses);

    // RFC 9110 section 11.6.1: a 401 MUST carry a challenge; only a scheme that
    // has one declares it. The `HeaderValue` test matches `AuthRejection`'s, so
    // an invalid challenge is absent from both the response and the description.
    if let Some(challenge) = challenge.filter(|value| HeaderValue::from_str(value).is_ok()) {
        operation.add_response_header(
            StatusPattern::Code(StatusCode::UNAUTHORIZED.as_u16()),
            "WWW-Authenticate",
            &Header::new(Schema::of_type(SchemaType::String))
                .required(true)
                .with_description(
                    "The challenge the client must answer, per RFC 9110 section 11.6.1.",
                )
                .with_example(challenge),
        );
    }

    operation.set_security(security);
}

#[cfg(test)]
mod tests;
