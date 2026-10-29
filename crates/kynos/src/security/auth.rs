//! The guards a handler takes, and the scopes they must carry.
//!
//! Taking one of these as a handler's [`Guard`] enforces the requirement, sets
//! the operation's `security`, and adds 401 and 403 to its `responses`, as one
//! act — and 503 where a scheme it names sets
//! [`MAY_BE_UNAVAILABLE`](crate::security::SecurityScheme::MAY_BE_UNAVAILABLE).
//! [`Auth`] and [`MaybeAuth`] leave the 403 out where no scheme they name sets
//! [`FORBIDS`](crate::security::SecurityScheme::FORBIDS).

use kynos_openapi::{
    Header, Schema, SecurityRequirement, StatusPattern, model::schema::types::SchemaType,
};

use crate::{
    error::rejection::{AuthRejection, ScopedRejection, auth_responses},
    extract::describe::Describe,
    http::{HeaderValue, Parts, StatusCode},
    router::operation::OperationCx,
    security::{
        Guard, Scope,
        requirement::{CheckedBy, Requirement, Scopable, ScopeCheckedBy},
        sealed,
    },
};

/// Implements `Clone`, `Copy`, `Debug`, `PartialEq` and `Eq` for a guard, and
/// its `into_inner`.
///
/// Hand-written so the bounds fall on the credential, not the marker scheme.
/// No `Default` (an unverified credential) and no `Ord`. `$new` rebuilds the
/// guard from a cloned credential.
macro_rules! credential_traits {
    (
        $guard:ident[$($generics:tt)*][$($args:tt)*], $new:expr;
        $(#[$meta:meta])*
        fn into_inner -> $inner:ty;
    ) => {
        impl<$($generics)*> Clone for $guard<$($args)*>
        where
            S::Credential: Clone,
        {
            fn clone(&self) -> Self {
                $new(self.0.clone())
            }
        }

        impl<$($generics)*> Copy for $guard<$($args)*> where S::Credential: Copy {}

        impl<$($generics)*> std::fmt::Debug for $guard<$($args)*>
        where
            S::Credential: std::fmt::Debug,
        {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_tuple(stringify!($guard)).field(&self.0).finish()
            }
        }

        impl<$($generics)*> PartialEq for $guard<$($args)*>
        where
            S::Credential: PartialEq,
        {
            fn eq(&self, other: &Self) -> bool {
                self.0 == other.0
            }
        }

        impl<$($generics)*> Eq for $guard<$($args)*> where S::Credential: Eq {}

        impl<$($generics)*> $guard<$($args)*> {
            $(#[$meta])*
            pub fn into_inner(self) -> $inner {
                self.0
            }
        }
    };
}

/// A credential proving the request satisfies requirement `S`.
///
/// Taking this as a handler's guard — its first argument — does three things at
/// once: it enforces the requirement, it sets the operation's `security` to
/// `S`'s, and it adds 401 and 403 to the operation's `responses`. There is no
/// way to do one without the others. The 403 is left out where no scheme `S`
/// names sets [`SecurityScheme::FORBIDS`](crate::security::SecurityScheme::FORBIDS).
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

credential_traits! {
    Auth[S: Requirement][S], Self;
    /// Unwraps the verified credential.
    fn into_inner -> S::Credential;
}

impl<S: Requirement> Describe for Auth<S> {
    /// Declares its 403, where [`Requirement::FORBIDS`] holds for `S`, as the
    /// shared, un-narrowed `Problem` component, since the authorizer may refuse
    /// with any problem type.
    ///
    /// The type on a narrowed 403 is [`Scopes::FORBIDDEN_TYPE`], and this
    /// argument names no scope set; [`Scoped`] is the guard that does.
    fn describe(operation: &mut OperationCx<'_>) {
        let security = S::declare(operation);
        declare_unscoped::<S>(operation, security);
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

credential_traits! {
    MaybeAuth[S: Requirement][S], Self;
    /// Unwraps the verified credential, if the request carried one.
    fn into_inner -> Option<S::Credential>;
}

impl<S: Requirement> Describe for MaybeAuth<S> {
    /// Declares its 403, where [`Requirement::FORBIDS`] holds for `S`, as the
    /// shared, un-narrowed `Problem` component: naming no scope set, it has no
    /// [`Scopes::FORBIDDEN_TYPE`] to narrow it to.
    fn describe(operation: &mut OperationCx<'_>) {
        let mut security = vec![SecurityRequirement::anonymous()];
        security.extend(S::declare(operation));
        declare_unscoped::<S>(operation, security);
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
/// [`Scoped<S, R>`](Scoped) argument holds to the
/// [`SecurityScheme::Scope`](crate::security::SecurityScheme::Scope) of every
/// scheme `S` names: a set written against another type does not compile.
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
    /// What each scope is: the
    /// [`SecurityScheme::Scope`](crate::security::SecurityScheme::Scope) of
    /// every scheme this set is demanded under.
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
/// `S` is a scheme, or an [`AnyOf`](crate::security::requirement::AnyOf) of
/// schemes sharing one scope type, which demands the set of whichever
/// alternative authenticates and declares it on each: `[{A: [r]}, {B: [r]}]`.
/// [`Scopable`] is what admits the two.
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
///
/// So does an [`AnyOf`](crate::security::requirement::AnyOf) holding one such
/// scheme, since the set is demanded of every alternative:
///
/// ```compile_fail,E0277
/// # use kynos::security::{Scope, auth::{Scoped, Scopes}, requirement::AnyOf};
/// # use kynos::security::schemes::{Basic, Bearer};
/// # #[derive(Clone, Copy)]
/// # enum Grant { ReadReports }
/// # impl Scope for Grant {
/// #     fn name(&self) -> &'static str { "reports:read" }
/// # }
/// # struct ReadReports;
/// # impl Scopes for ReadReports {
/// #     type Scope = Grant;
/// #     const REQUIRED: &'static [Grant] = &[Grant::ReadReports];
/// # }
/// async fn reports(caller: Scoped<AnyOf<(Bearer<String, Grant>, Basic)>, ReadReports>) {}
/// ```
///
/// An [`AllOf`](crate::security::requirement::AllOf) is not [`Scopable`]:
///
/// ```compile_fail,E0277
/// # use kynos::security::{Scope, auth::{Scoped, Scopes}, requirement::AllOf};
/// # use kynos::security::schemes::{Basic, Bearer};
/// # #[derive(Clone, Copy)]
/// # enum Grant { ReadReports }
/// # impl Scope for Grant {
/// #     fn name(&self) -> &'static str { "reports:read" }
/// # }
/// # struct ReadReports;
/// # impl Scopes for ReadReports {
/// #     type Scope = Grant;
/// #     const REQUIRED: &'static [Grant] = &[Grant::ReadReports];
/// # }
/// async fn reports(
///     caller: Scoped<AllOf<(Bearer<String, Grant>, Basic<String, Grant>)>, ReadReports>,
/// ) {
/// }
/// ```
pub struct Scoped<S: Scopable, R: Scopes<Scope = S::Scope>>(
    pub S::Credential,
    std::marker::PhantomData<R>,
);

credential_traits! {
    Scoped[S: Scopable, R: Scopes<Scope = S::Scope>][S, R], Self::new;
    /// Unwraps the verified and authorized credential.
    #[must_use]
    fn into_inner -> S::Credential;
}

impl<S: Scopable, R: Scopes<Scope = S::Scope>> Scoped<S, R> {
    /// Wraps a credential that has been verified and authorized.
    ///
    /// The marker field is private, so this is how one is built from outside
    /// the crate.
    pub fn new(credential: S::Credential) -> Self {
        Self(credential, std::marker::PhantomData)
    }
}

impl<S: Scopable, R: Scopes<Scope = S::Scope>> Describe for Scoped<S, R> {
    fn describe(operation: &mut OperationCx<'_>) {
        let security = S::declare_scoped(operation, R::REQUIRED);
        // `Scoped` always authorizes, so each scheme may always refuse a scope.
        let challenges = Challenges {
            unauthenticated: S::challenge(),
            insufficient_scope: S::scope_challenge(),
        };
        // Nor can a scheme that never forbids drop the 403 a scope refusal is.
        let refusals = Refusals {
            forbidden_type: R::FORBIDDEN_TYPE,
            forbids: true,
            unavailable: S::MAY_BE_UNAVAILABLE,
        };
        declare(operation, challenges, security, refusals);
    }
}

impl<S: Scopable, R: Scopes<Scope = S::Scope>> sealed::Sealed for Scoped<S, R> {}

impl<C, S, R> Guard<C> for Scoped<S, R>
where
    C: Sync,
    S: ScopeCheckedBy<C>,
    R: Scopes<Scope = S::Scope>,
{
    /// [`AuthRejection`] described against `R`, which is what narrows the 403
    /// this argument declares.
    type Rejection = ScopedRejection<R>;

    async fn guard(parts: &Parts, context: &C) -> Result<Self, Self::Rejection> {
        // Every refusal is challenged: an `authorize` answering 401 owes one
        // too. An absent credential is `Scoped`'s own 401.
        S::check_scoped(parts, context, R::REQUIRED)
            .await
            .and_then(|checked| checked.ok_or_else(AuthRejection::unauthenticated))
            .map(|credential| Self(credential, std::marker::PhantomData))
            .map_err(|rejection| ScopedRejection::new(rejection.with_challenge(S::challenge())))
    }
}

/// Sets the operation's `security` to the guard's, and declares how it refuses.
///
/// `security` is the guard's whole list, set once rather than appended to.
fn declare(
    operation: &mut OperationCx<'_>,
    challenges: Challenges,
    security: Vec<SecurityRequirement>,
    refusals: Refusals,
) {
    // Before the header: `add_response_header` invents a thin 401 when none is
    // declared, and merging cannot replace an existing response.
    let responses = auth_responses(
        operation.registry(),
        refusals.forbidden_type,
        refusals.forbids,
        refusals.unavailable,
    );
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
    // the credential rather than a scope, and sends none. No 403 is declared
    // without `forbids`, and the header would invent one.
    if refusals.forbids {
        declare_challenge(
            operation,
            StatusCode::FORBIDDEN,
            challenges.insufficient_scope,
            Header::new(Schema::of_type(SchemaType::String)).with_description(
                "Sent when the credential lacks a required scope, per RFC 6750 section 3.1.",
            ),
        );
    }

    operation.set_security(security);
}

/// [`declare`] for a guard naming no scope set: `S`'s challenges and
/// refusals, with the 403 un-narrowed.
fn declare_unscoped<S: Requirement>(
    operation: &mut OperationCx<'_>,
    security: Vec<SecurityRequirement>,
) {
    let challenges = Challenges {
        unauthenticated: S::challenge(),
        insufficient_scope: S::insufficient_scope_challenge(),
    };
    let refusals = Refusals {
        forbidden_type: None,
        forbids: S::FORBIDS,
        unavailable: S::MAY_BE_UNAVAILABLE,
    };
    declare(operation, challenges, security, refusals);
}

/// Which refusals beside the 401 a guard declares, as [`declare`] declares
/// them.
#[derive(Clone, Copy)]
struct Refusals {
    /// [`Scopes::FORBIDDEN_TYPE`] where the argument named a scope set, and
    /// `None` where it named only a scheme.
    forbidden_type: Option<&'static str>,
    /// Whether the 403 is declared: [`Requirement::FORBIDS`] for a guard
    /// naming no scope set, and always for one that does.
    forbids: bool,
    /// [`Requirement::MAY_BE_UNAVAILABLE`], which adds the 503.
    unavailable: bool,
}

/// The challenges a guard's refusals carry, as [`declare`] declares them.
#[derive(Clone, Copy)]
struct Challenges {
    /// The 401's: [`Requirement::challenge`].
    unauthenticated: Option<&'static str>,
    /// A scope-refusing 403's, where the guard authorizes a scope at all:
    /// [`Requirement::insufficient_scope_challenge`] for a guard naming no
    /// scope set, and [`Scopable::scope_challenge`] for one that does.
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
