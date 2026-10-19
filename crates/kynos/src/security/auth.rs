//! The guards a handler takes, and the scopes they must carry.
//!
//! Taking one of these as a handler's [`Guard`] enforces the requirement, sets
//! the operation's `security`, and adds 401 and 403 to its `responses`, as one
//! act.

use std::borrow::Cow;

use kynos_openapi::{
    Header, Schema, SecurityRequirement, StatusPattern, model::schema::types::SchemaType,
};

use crate::{
    error::rejection::{AuthRejection, ScopedRejection, auth_responses},
    extract::describe::Describe,
    http::{HeaderValue, Parts, StatusCode},
    router::operation::OperationCx,
    security::{
        Authenticates, Guard, Scope, SecurityScheme,
        carrier::Carries,
        requirement::{CheckedBy, Requirement, require, verify},
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
/// #     type Scope = kynos::security::Unscoped;
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
        let challenges = Challenges {
            unauthenticated: S::challenge(),
            insufficient_scope: S::insufficient_scope_challenge(),
        };
        declare(operation, challenges, security, None);
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
/// anonymity — and a scheme derived with `carrier = optional` reports none, so
/// under it the authenticator decides every request and what it admits is
/// always `Some`.
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
/// #     type Scope = kynos::security::Unscoped;
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
        let challenges = Challenges {
            unauthenticated: S::challenge(),
            insufficient_scope: S::insufficient_scope_challenge(),
        };
        declare(operation, challenges, security, None);
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
/// repeated across handlers. Its scopes are values of a [`Scope`] type, which a
/// [`Scoped<S, R>`](Scoped) argument holds to scheme `S`'s own
/// [`SecurityScheme::Scope`]: a set written against another type does not
/// compile.
///
/// ```
/// use kynos::security::{Scope, auth::Scopes};
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
/// /// What an administrative endpoint demands, and what refusing it is called.
/// struct Admin;
///
/// impl Scopes for Admin {
///     type Scope = Grant;
///     const REQUIRED: &'static [Grant] = &[Grant::Admin];
///     const FORBIDDEN_TYPE: Option<&'static str> =
///         Some("https://errors.example.com/insufficient-scope");
/// }
///
/// /// A scope set naming no problem type, which is still the ordinary case.
/// struct ReadReports;
///
/// impl Scopes for ReadReports {
///     type Scope = Grant;
///     const REQUIRED: &'static [Grant] = &[Grant::ReadReports];
/// }
///
/// assert_eq!(ReadReports::FORBIDDEN_TYPE, None);
/// ```
pub trait Scopes: Send + Sync + 'static {
    /// What each scope is: the [`SecurityScheme::Scope`] of every scheme this
    /// set is demanded under.
    type Scope: Scope;

    /// The scopes required.
    const REQUIRED: &'static [Self::Scope];

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
    /// `about:blank` — the whole guard, so an
    /// [`Authenticator::authenticate`](crate::security::Authenticator::authenticate)
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
/// The scope set is a type implementing [`Scopes`], since a slice is not a
/// permitted const parameter type, and its [`Scope`](Scopes::Scope) must be
/// `S`'s. The marker field is private, so outside this crate a handler cannot
/// destructure `Scoped(claims)` as it does [`Auth`]: it reads the credential
/// through `.0` or [`into_inner`](Self::into_inner).
///
/// ```no_run
/// # use kynos::security::{Scope, auth::{Scoped, Scopes}, schemes::Bearer};
/// # #[derive(Clone, Copy)]
/// # enum Grant { ReadReports }
/// # impl Scope for Grant {
/// #     fn name(&self) -> &'static str { "reports:read" }
/// # }
/// struct ReadReports;
///
/// impl Scopes for ReadReports {
///     type Scope = Grant;
///     const REQUIRED: &'static [Grant] = &[Grant::ReadReports];
/// }
///
/// async fn reports(caller: Scoped<Bearer<String, Grant>, ReadReports>) {}
/// ```
///
/// A scheme whose scope type is another, here the default
/// [`Unscoped`](crate::security::Unscoped), refuses the set:
///
/// ```compile_fail
/// # use kynos::security::{Scope, auth::{Scoped, Scopes}, schemes::Bearer};
/// # #[derive(Clone, Copy)]
/// # enum Grant { ReadReports }
/// # impl Scope for Grant {
/// #     fn name(&self) -> &'static str { "reports:read" }
/// # }
/// struct ReadReports;
///
/// impl Scopes for ReadReports {
///     type Scope = Grant;
///     const REQUIRED: &'static [Grant] = &[Grant::ReadReports];
/// }
///
/// async fn reports(caller: Scoped<Bearer<String>, ReadReports>) {}
/// ```
pub struct Scoped<S: SecurityScheme, R: Scopes<Scope = S::Scope>>(
    pub S::Credential,
    std::marker::PhantomData<R>,
);

// As on `Auth`: bounded on the credential, without `Default`.
impl<S: SecurityScheme, R: Scopes<Scope = S::Scope>> Clone for Scoped<S, R>
where
    S::Credential: Clone,
{
    fn clone(&self) -> Self {
        Self(self.0.clone(), std::marker::PhantomData)
    }
}

impl<S: SecurityScheme, R: Scopes<Scope = S::Scope>> Copy for Scoped<S, R> where S::Credential: Copy {}

impl<S: SecurityScheme, R: Scopes<Scope = S::Scope>> std::fmt::Debug for Scoped<S, R>
where
    S::Credential: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Scoped").field(&self.0).finish()
    }
}

impl<S: SecurityScheme, R: Scopes<Scope = S::Scope>> PartialEq for Scoped<S, R>
where
    S::Credential: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<S: SecurityScheme, R: Scopes<Scope = S::Scope>> Eq for Scoped<S, R> where S::Credential: Eq {}

impl<S: SecurityScheme, R: Scopes<Scope = S::Scope>> Scoped<S, R> {
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

impl<S: SecurityScheme, R: Scopes<Scope = S::Scope>> Describe for Scoped<S, R> {
    fn describe(operation: &mut OperationCx<'_>) {
        // The scopes the guard authorizes, without naming one twice.
        let mut scopes = Vec::new();
        for scope in demanded::<S, R>().iter().map(Scope::name) {
            if !scopes.contains(&scope) {
                scopes.push(scope);
            }
        }
        let security = vec![require::<S>(operation, scopes)];
        // `Scoped` always authorizes, so its scheme may always refuse a scope.
        let challenges = Challenges {
            unauthenticated: <S as SecurityScheme>::challenge(),
            insufficient_scope: <S as SecurityScheme>::insufficient_scope_challenge(),
        };
        declare(operation, challenges, security, R::FORBIDDEN_TYPE);
    }
}

impl<S: SecurityScheme, R: Scopes<Scope = S::Scope>> sealed::Sealed for Scoped<S, R> {}

impl<C, S, R> Guard<C> for Scoped<S, R>
where
    C: Authenticates<S>,
    S: Carries,
    R: Scopes<Scope = S::Scope>,
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

        let credential = verify::<S, C>(presented, Some(&demanded::<S, R>()), context)
            .await
            .map_err(challenged)?;
        Ok(Self(credential, std::marker::PhantomData))
    }
}

/// The scopes a [`Scoped<S, R>`](Scoped) argument demands: `S`'s defaults,
/// then each of `R`'s that the defaults do not already name.
///
/// `R`'s scopes join the defaults rather than replacing them, so the scheme's
/// demand holds under every guard. Borrowed, and so free, when `S` has none.
fn demanded<S, R>() -> Cow<'static, [S::Scope]>
where
    S: SecurityScheme,
    R: Scopes<Scope = S::Scope>,
{
    let defaults = S::scopes();
    if defaults.is_empty() {
        return Cow::Borrowed(R::REQUIRED);
    }

    let mut union = defaults.to_vec();
    for scope in R::REQUIRED {
        if !union.iter().any(|held| held.name() == scope.name()) {
            union.push(scope.clone());
        }
    }
    Cow::Owned(union)
}

/// Sets the operation's `security` to the guard's, and declares how it refuses.
///
/// `security` is the guard's whole list, set once rather than appended to.
/// `forbidden_type` is [`Scopes::FORBIDDEN_TYPE`] where the argument named a
/// scope set, and `None` where it named only a scheme.
fn declare(
    operation: &mut OperationCx<'_>,
    challenges: Challenges,
    security: Vec<SecurityRequirement>,
    forbidden_type: Option<&'static str>,
) {
    // Before the header: `add_response_header` invents a thin 401 when none is
    // declared, and merging cannot replace an existing response.
    let responses = auth_responses(operation.registry(), forbidden_type);
    operation.add_responses(&responses);

    // RFC 9110 section 15.5.2: a 401 MUST carry a challenge. A scheme that
    // declares none -- one implemented by hand, or a derived `http` kind not
    // told its parameters -- has nothing to declare.
    declare_challenge(
        operation,
        StatusCode::UNAUTHORIZED,
        challenges.unauthenticated,
        Header::new(Schema::of_type(SchemaType::String))
            .required(true)
            .with_description("The challenge the client must answer, per RFC 9110 section 11.6.1."),
    );

    // RFC 6750 section 3.1: a bearer token lacking a scope is answered with
    // `insufficient_scope`. Not required: a 403 `authenticate` answers declines
    // the credential rather than a scope, and sends none.
    declare_challenge(
        operation,
        StatusCode::FORBIDDEN,
        challenges.insufficient_scope,
        Header::new(Schema::of_type(SchemaType::String)).with_description(
            "Sent when the credential lacks a required scope, per RFC 6750 section 3.1.",
        ),
    );

    operation.set_security(security);
}

/// The challenges a guard's refusals carry, as [`declare`] declares them.
#[derive(Clone, Copy)]
struct Challenges {
    /// The 401's: [`SecurityScheme::challenge`].
    unauthenticated: Option<&'static str>,
    /// A scope-refusing 403's:
    /// [`SecurityScheme::insufficient_scope_challenge`], where the guard
    /// authorizes a scope at all.
    insufficient_scope: Option<&'static str>,
}

/// Declares `challenge` as the `WWW-Authenticate` that `status` carries.
///
/// The `HeaderValue` test matches `AuthRejection`'s, so an invalid challenge is
/// absent from both the response and the description.
fn declare_challenge(
    operation: &mut OperationCx<'_>,
    status: StatusCode,
    challenge: Option<&'static str>,
    header: Header,
) {
    if let Some(challenge) = challenge.filter(|value| HeaderValue::from_str(value).is_ok()) {
        operation.add_response_header(
            StatusPattern::Code(status.as_u16()),
            "WWW-Authenticate",
            &header.with_example(challenge),
        );
    }
}

#[cfg(test)]
mod tests;
