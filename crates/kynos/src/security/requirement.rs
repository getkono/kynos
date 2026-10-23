//! What a guard demands: one scheme, or several schemes combined.
//!
//! OpenAPI spells an operation's `security` as a list of alternatives, each a
//! set of schemes that must all be satisfied together. A guard's type parameter
//! is that list, written as a type, and the guard emits the whole of it at
//! once:
//!
//! | Parameter | `security` | Runs |
//! | --- | --- | --- |
//! | `S` | `[{S}]` | `S` |
//! | [`AnyOf<(A, B)>`](AnyOf) | `[{A}, {B}]` | `A`, then `B`; the first that is satisfied wins |
//! | [`AllOf<(A, B)>`](AllOf) | `[{A, B}]` | both, and both must be satisfied |
//!
//! A scheme is satisfied once it authenticates and is authorized for its
//! default scopes, [`SecurityScheme::scopes`], where it has any.
//!
//! [`MaybeAuth`](crate::security::auth::MaybeAuth) prepends `{}` to any of the
//! three. A combinator takes schemes and not other combinators, so each shape
//! has exactly one spelling.
//!
//! [`Scoped`](crate::security::auth::Scoped) demands its scope set of a
//! scheme, or of every alternative of an [`AnyOf`] whose schemes share one
//! [`SecurityScheme::Scope`]: [`Scopable`] is what admits the two.

use std::{borrow::Cow, future::Future, marker::PhantomData};

use kynos_openapi::{ComponentName, SecurityRequirement};

use crate::{
    error::rejection::AuthRejection,
    http::Parts,
    router::operation::OperationCx,
    security::{Authenticates, Authenticator, Scope, SecurityScheme, carrier::Carries},
};

/// What closes the set of requirements.
mod sealed {
    /// The private supertrait.
    pub trait Sealed {}
}

/// What a guard demands, as a type: a [`SecurityScheme`], an [`AnyOf`] or an
/// [`AllOf`].
///
/// Sealed. Every scheme is one through a blanket implementation, and the two
/// combinators are the only other shapes.
pub trait Requirement: sealed::Sealed + Send + Sync + 'static {
    /// What satisfying the requirement yields the handler.
    type Credential: Send;

    /// Whether a guard over this requirement may answer 503: any scheme's
    /// [`SecurityScheme::MAY_BE_UNAVAILABLE`], since each one's authenticator
    /// is consulted.
    const MAY_BE_UNAVAILABLE: bool;

    /// Whether a guard over this requirement may answer 403: any scheme's
    /// [`SecurityScheme::FORBIDS`], since each one's authenticator is
    /// consulted.
    const FORBIDS: bool;

    /// The `WWW-Authenticate` challenge a 401 refusing this requirement sends.
    ///
    /// The first scheme's that has one, in declaration order: a 401 needs only
    /// one challenge a client can answer.
    fn challenge() -> Option<&'static str>;

    /// The `WWW-Authenticate` challenge a 403 refusing this requirement's
    /// default scopes may send.
    ///
    /// The first scheme's that has default scopes and an
    /// [`insufficient_scope_challenge`](SecurityScheme::insufficient_scope_challenge),
    /// in declaration order: a scheme with no defaults is never authorized by
    /// a guard naming no scope set, so it never refuses a scope.
    fn insufficient_scope_challenge() -> Option<&'static str>;

    /// Registers every scheme named, and returns the alternatives in the order
    /// the operation's `security` lists them.
    fn declare(operation: &mut OperationCx<'_>) -> Vec<SecurityRequirement>;
}

/// A [`Requirement`] the context `C` can check against a request.
///
/// Implemented wherever `C` implements [`Authenticates`] for every scheme the
/// requirement names, so a router guarding an operation with it cannot be built
/// on a context that could not verify it.
pub trait CheckedBy<C>: Requirement {
    /// Reads and verifies the credential the request presented.
    ///
    /// `Ok(None)` is *absent*: nothing this requirement reads was presented.
    /// The challenge is left unset, for the guard to attach.
    fn check(
        parts: &Parts,
        context: &C,
    ) -> impl Future<Output = Result<Option<Self::Credential>, AuthRejection>> + Send;
}

/// A [`Requirement`] a [`Scoped`](crate::security::auth::Scoped) guard can
/// demand a scope set of: a [`SecurityScheme`], or an [`AnyOf`] whose schemes
/// share one [`SecurityScheme::Scope`].
///
/// An `AnyOf` demands the set of whichever alternative authenticates, and
/// declares it on each, `[{A: [r]}, {B: [r]}]`. An [`AllOf`] is not one: a set
/// it demanded would be demanded of every scheme in it.
///
/// Sealed, as [`Requirement`] is.
pub trait Scopable: Requirement {
    /// What each scope is: the [`SecurityScheme::Scope`] of every scheme named.
    type Scope: Scope;

    /// The `WWW-Authenticate` challenge a 403 refusing a demanded scope may
    /// send.
    ///
    /// The first scheme's
    /// [`insufficient_scope_challenge`](SecurityScheme::insufficient_scope_challenge),
    /// in declaration order: a guard demanding a scope set authorizes every
    /// scheme, so each may refuse a scope.
    fn scope_challenge() -> Option<&'static str>;

    /// Registers every scheme named, and returns the alternatives, each
    /// carrying its scheme's default scopes followed by each of `required`
    /// they do not already name.
    fn declare_scoped(
        operation: &mut OperationCx<'_>,
        required: &'static [Self::Scope],
    ) -> Vec<SecurityRequirement>;
}

/// A [`Scopable`] requirement the context `C` can check against a request,
/// together with a scope set.
///
/// Implemented wherever `C` implements [`Authenticates`] for every scheme the
/// requirement names, as [`CheckedBy`] is.
pub trait ScopeCheckedBy<C>: Scopable {
    /// Reads and verifies the credential the request presented, and authorizes
    /// it for its scheme's default scopes and `required` in one call.
    ///
    /// `Ok(None)` is *absent*, as in [`CheckedBy::check`]. A presented
    /// credential is always authorized, even for an empty demand.
    fn check_scoped(
        parts: &Parts,
        context: &C,
        required: &'static [Self::Scope],
    ) -> impl Future<Output = Result<Option<Self::Credential>, AuthRejection>> + Send;
}

impl<S: SecurityScheme> sealed::Sealed for S {}

impl<S: SecurityScheme> Requirement for S {
    type Credential = <S as SecurityScheme>::Credential;

    const MAY_BE_UNAVAILABLE: bool = <S as SecurityScheme>::MAY_BE_UNAVAILABLE;

    const FORBIDS: bool = <S as SecurityScheme>::FORBIDS;

    fn challenge() -> Option<&'static str> {
        <S as SecurityScheme>::challenge()
    }

    fn insufficient_scope_challenge() -> Option<&'static str> {
        owed_scope_challenge::<S>()
    }

    fn declare(operation: &mut OperationCx<'_>) -> Vec<SecurityRequirement> {
        vec![require::<S>(operation, defaults::<S>())]
    }
}

impl<C, S> CheckedBy<C> for S
where
    S: Carries,
    C: Authenticates<S>,
{
    async fn check(parts: &Parts, context: &C) -> Result<Option<S::Credential>, AuthRejection> {
        // Only a presented credential reaches the verifier.
        let Some(presented) = S::present(parts)? else {
            return Ok(None);
        };

        verify::<S, C>(presented, owed_defaults::<S>(), context)
            .await
            .map(Some)
    }
}

impl<S: SecurityScheme> Scopable for S {
    type Scope = <S as SecurityScheme>::Scope;

    fn scope_challenge() -> Option<&'static str> {
        <S as SecurityScheme>::insufficient_scope_challenge()
    }

    fn declare_scoped(
        operation: &mut OperationCx<'_>,
        required: &'static [<S as SecurityScheme>::Scope],
    ) -> Vec<SecurityRequirement> {
        vec![require::<S>(operation, demanded_names::<S>(required))]
    }
}

impl<C, S> ScopeCheckedBy<C> for S
where
    S: Carries,
    C: Authenticates<S>,
{
    async fn check_scoped(
        parts: &Parts,
        context: &C,
        required: &'static [<S as SecurityScheme>::Scope],
    ) -> Result<Option<S::Credential>, AuthRejection> {
        let Some(presented) = S::present(parts)? else {
            return Ok(None);
        };

        verify::<S, C>(presented, Some(&demanded::<S>(required)), context)
            .await
            .map(Some)
    }
}

/// Any one of a tuple of schemes, tried in order.
///
/// Declares one requirement per scheme, `[{A}, {B}]`, which is OpenAPI's
/// "either". The first scheme that authenticates, and is authorized for its
/// default scopes, wins; a scheme whose credential is absent is skipped. When
/// none is satisfied, the first refusal in declaration order is the answer,
/// and when nothing was presented at all the requirement is absent.
///
/// Implemented for tuples of two to four schemes. The credential is an
/// [`Either2`], [`Either3`] or [`Either4`] naming which one authenticated.
///
/// ```no_run
/// # use kynos::security::{auth::Auth, requirement::{AnyOf, Either2}, schemes::{Basic, Bearer}};
/// async fn me(Auth(caller): Auth<AnyOf<(Bearer, Basic)>>) {
///     match caller {
///         Either2::First(token) => todo!("a bearer token"),
///         Either2::Second(credentials) => todo!("a username and password"),
///     }
/// }
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnyOf<T>(PhantomData<fn() -> T>);

/// Every one of a tuple of schemes, together.
///
/// Declares a single requirement naming all of them, `[{A, B}]`, which is
/// OpenAPI's "both". Every credential is read before any is verified: when none
/// was presented the requirement is absent, and when only some were it is a
/// 401, because a partial set is not an anonymous request. Every scheme is
/// authenticated before any is authorized for its default scopes, so a
/// credential that fails to authenticate is a 401 whatever another scheme's
/// scopes would have answered.
///
/// Implemented for tuples of two to four schemes. The credential is the tuple
/// of theirs, in the same order.
///
/// ```no_run
/// # use kynos::security::{auth::Auth, requirement::AllOf, schemes::{Bearer, MutualTls}};
/// async fn transfer(Auth((token, certificate)): Auth<AllOf<(Bearer, MutualTls)>>) {
///     todo!()
/// }
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AllOf<T>(PhantomData<fn() -> T>);

/// Which of two schemes in an [`AnyOf`] authenticated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Either2<A, B> {
    /// The first scheme's credential.
    First(A),
    /// The second scheme's credential.
    Second(B),
}

/// Which of three schemes in an [`AnyOf`] authenticated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Either3<A, B, C> {
    /// The first scheme's credential.
    First(A),
    /// The second scheme's credential.
    Second(B),
    /// The third scheme's credential.
    Third(C),
}

/// Which of four schemes in an [`AnyOf`] authenticated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Either4<A, B, C, D> {
    /// The first scheme's credential.
    First(A),
    /// The second scheme's credential.
    Second(B),
    /// The third scheme's credential.
    Third(C),
    /// The fourth scheme's credential.
    Fourth(D),
}

/// The first challenge among `challenges`, in order.
fn first_challenge(challenges: &[fn() -> Option<&'static str>]) -> Option<&'static str> {
    challenges.iter().find_map(|challenge| challenge())
}

/// Implements [`Requirement`], [`CheckedBy`], [`Scopable`] and
/// [`ScopeCheckedBy`] for one [`AnyOf`] arity.
///
/// The first scheme is named apart, `$first`, so the others' scope types can
/// be bound to its.
macro_rules! any_of {
    ($either:ident; $first:ident; $($scheme:ident => $variant:ident),+; $($rest:ident),+) => {
        impl<$($scheme: SecurityScheme),+> sealed::Sealed for AnyOf<($($scheme,)+)> {}

        impl<$($scheme: SecurityScheme),+> Requirement for AnyOf<($($scheme,)+)> {
            type Credential = $either<$(<$scheme as SecurityScheme>::Credential),+>;

            const MAY_BE_UNAVAILABLE: bool =
                false $(|| <$scheme as SecurityScheme>::MAY_BE_UNAVAILABLE)+;

            const FORBIDS: bool = false $(|| <$scheme as SecurityScheme>::FORBIDS)+;

            fn challenge() -> Option<&'static str> {
                first_challenge(&[$(<$scheme as SecurityScheme>::challenge),+])
            }

            fn insufficient_scope_challenge() -> Option<&'static str> {
                first_challenge(&[$(owed_scope_challenge::<$scheme>),+])
            }

            fn declare(operation: &mut OperationCx<'_>) -> Vec<SecurityRequirement> {
                // Two schemes registered under one key are one alternative.
                let mut alternatives = Vec::new();
                $(
                    let requirement = require::<$scheme>(operation, defaults::<$scheme>());
                    if !alternatives.contains(&requirement) {
                        alternatives.push(requirement);
                    }
                )+
                alternatives
            }
        }

        impl<C, $($scheme),+> CheckedBy<C> for AnyOf<($($scheme,)+)>
        where
            $($scheme: Carries,)+
            $(C: Authenticates<$scheme>,)+
        {
            async fn check(
                parts: &Parts,
                context: &C,
            ) -> Result<Option<Self::Credential>, AuthRejection> {
                let mut refused = None;
                $(
                    match <$scheme as CheckedBy<C>>::check(parts, context).await {
                        Ok(Some(credential)) => return Ok(Some($either::$variant(credential))),
                        Ok(None) => {}
                        Err(rejection) => {
                            refused.get_or_insert(rejection);
                        }
                    }
                )+
                refused.map_or(Ok(None), Err)
            }
        }

        impl<$first, $($rest),+> Scopable for AnyOf<($first, $($rest,)+)>
        where
            $first: SecurityScheme,
            $($rest: SecurityScheme<Scope = <$first as SecurityScheme>::Scope>,)+
        {
            type Scope = <$first as SecurityScheme>::Scope;

            fn scope_challenge() -> Option<&'static str> {
                first_challenge(&[$(<$scheme as SecurityScheme>::insufficient_scope_challenge),+])
            }

            fn declare_scoped(
                operation: &mut OperationCx<'_>,
                required: &'static [<$first as SecurityScheme>::Scope],
            ) -> Vec<SecurityRequirement> {
                // As `declare`: two schemes under one key are one alternative.
                let mut alternatives = Vec::new();
                $(
                    let requirement =
                        require::<$scheme>(operation, demanded_names::<$scheme>(required));
                    if !alternatives.contains(&requirement) {
                        alternatives.push(requirement);
                    }
                )+
                alternatives
            }
        }

        impl<C, $first, $($rest),+> ScopeCheckedBy<C> for AnyOf<($first, $($rest,)+)>
        where
            $first: Carries,
            $($rest: Carries<Scope = <$first as SecurityScheme>::Scope>,)+
            $(C: Authenticates<$scheme>,)+
        {
            async fn check_scoped(
                parts: &Parts,
                context: &C,
                required: &'static [<$first as SecurityScheme>::Scope],
            ) -> Result<Option<Self::Credential>, AuthRejection> {
                // `check`'s order and precedence, with `required` demanded of
                // whichever alternative is tried.
                let mut refused = None;
                $(
                    match <$scheme as ScopeCheckedBy<C>>::check_scoped(parts, context, required)
                        .await
                    {
                        Ok(Some(credential)) => return Ok(Some($either::$variant(credential))),
                        Ok(None) => {}
                        Err(rejection) => {
                            refused.get_or_insert(rejection);
                        }
                    }
                )+
                refused.map_or(Ok(None), Err)
            }
        }
    };
}

/// Implements [`Requirement`] and [`CheckedBy`] for one [`AllOf`] arity.
macro_rules! all_of {
    ($($scheme:ident),+) => {
        impl<$($scheme: SecurityScheme),+> sealed::Sealed for AllOf<($($scheme,)+)> {}

        impl<$($scheme: SecurityScheme),+> Requirement for AllOf<($($scheme,)+)> {
            type Credential = ($(<$scheme as SecurityScheme>::Credential,)+);

            const MAY_BE_UNAVAILABLE: bool =
                false $(|| <$scheme as SecurityScheme>::MAY_BE_UNAVAILABLE)+;

            const FORBIDS: bool = false $(|| <$scheme as SecurityScheme>::FORBIDS)+;

            fn challenge() -> Option<&'static str> {
                first_challenge(&[$(<$scheme as SecurityScheme>::challenge),+])
            }

            fn insufficient_scope_challenge() -> Option<&'static str> {
                first_challenge(&[$(owed_scope_challenge::<$scheme>),+])
            }

            fn declare(operation: &mut OperationCx<'_>) -> Vec<SecurityRequirement> {
                let mut together = SecurityRequirement::anonymous();
                $(
                    together
                        .0
                        .extend(require::<$scheme>(operation, defaults::<$scheme>()).0);
                )+
                vec![together]
            }
        }

        impl<C, $($scheme),+> CheckedBy<C> for AllOf<($($scheme,)+)>
        where
            $($scheme: Carries,)+
            $(C: Authenticates<$scheme>,)+
        {
            #[allow(non_snake_case)]
            async fn check(
                parts: &Parts,
                context: &C,
            ) -> Result<Option<Self::Credential>, AuthRejection> {
                // Every carrier before any verifier, so a malformed or missing
                // credential costs no verification of the others.
                $( let $scheme = <$scheme as Carries>::present(parts)?; )+
                if [$($scheme.is_none()),+].into_iter().all(std::convert::identity) {
                    return Ok(None);
                }
                $(
                    let Some($scheme) = $scheme else {
                        return Err(AuthRejection::unauthenticated());
                    };
                )+
                // Every authentication before any authorization, so a request
                // one scheme cannot authenticate is a 401, never a 403, and is
                // never authorized.
                $( let $scheme = authenticate::<$scheme, C>($scheme, context).await?; )+
                $(
                    let $scheme =
                        authorize::<$scheme, C>($scheme, owed_defaults::<$scheme>(), context)
                            .await?;
                )+
                Ok(Some(($($scheme,)+)))
            }
        }
    };
}

any_of!(Either2; A; A => First, B => Second; B);
any_of!(Either3; A; A => First, B => Second, D => Third; B, D);
any_of!(Either4; A; A => First, B => Second, D => Third, E => Fourth; B, D, E);

all_of!(A, B);
all_of!(A, B, D);
all_of!(A, B, D, E);

/// Authenticates `presented` as scheme `S`, then authorizes it for `scopes`
/// where they are given.
///
/// `None` skips `authorize`, as a guard naming no scope set does for a scheme
/// with no default scopes ([`owed_defaults`]); `Some` calls it even with an
/// empty slice, as [`Scoped`](crate::security::auth::Scoped) does.
pub(crate) async fn verify<S, C>(
    presented: S::Presented,
    scopes: Option<&[S::Scope]>,
    context: &C,
) -> Result<S::Credential, AuthRejection>
where
    S: Carries,
    C: Authenticates<S>,
{
    let credential = authenticate::<S, C>(presented, context).await?;
    authorize::<S, C>(credential, scopes, context).await
}

/// Authenticates `presented` as scheme `S`.
///
/// A 403 here declines a credential rather than a scope, so it sends no
/// challenge, whatever the authenticator set.
async fn authenticate<S, C>(
    presented: S::Presented,
    context: &C,
) -> Result<S::Credential, AuthRejection>
where
    S: Carries,
    C: Authenticates<S>,
{
    context
        .authenticator()
        .authenticate(presented, context)
        .await
        .map_err(|rejection| rejection.with_scope_challenge(None))
}

/// Authorizes `credential` for scheme `S`'s `scopes` where they are given, and
/// hands it back.
///
/// A 403 here refused a scope, so it carries
/// [`SecurityScheme::insufficient_scope_challenge`]. Owned rather than
/// borrowed, so the future needs no `Sync` credential to be `Send`.
async fn authorize<S, C>(
    credential: S::Credential,
    scopes: Option<&[S::Scope]>,
    context: &C,
) -> Result<S::Credential, AuthRejection>
where
    S: Carries,
    C: Authenticates<S>,
{
    if let Some(scopes) = scopes {
        context
            .authenticator()
            .authorize(&credential, scopes, context)
            .await
            // The scheme's own, not `Requirement`'s, which only a guard naming
            // no scope set may declare.
            .map_err(|rejection| {
                rejection
                    .with_scope_challenge(<S as SecurityScheme>::insufficient_scope_challenge())
            })?;
    }
    Ok(credential)
}

/// What a guard naming no scope set authorizes for scheme `S`: its default
/// scopes, so the guard enforces what [`declare`](Requirement::declare) wrote
/// for it. `None` for a scheme with none, which is never authorized: its
/// requirement declares none, so there is nothing to check.
fn owed_defaults<S: SecurityScheme>() -> Option<&'static [S::Scope]> {
    Some(S::scopes()).filter(|scopes| !scopes.is_empty())
}

/// The challenge a guard naming no scope set may send on a 403 for scheme
/// `S`: its insufficient-scope challenge, where [`owed_defaults`] authorizes
/// it at all.
fn owed_scope_challenge<S: SecurityScheme>() -> Option<&'static str> {
    owed_defaults::<S>().and_then(|_| S::insufficient_scope_challenge())
}

/// Registers scheme `S` and returns the requirement naming it with `scopes`.
///
/// One name for both, so the requirement and the definition share a key.
pub(crate) fn require<S: SecurityScheme>(
    operation: &mut OperationCx<'_>,
    scopes: Vec<&'static str>,
) -> SecurityRequirement {
    let name = component_name::<S>();
    let requirement = SecurityRequirement::scoped(name.as_str(), scopes);
    operation.add_security_scheme(name, S::describe());
    requirement
}

/// The names of the scopes scheme `S` demands by default.
pub(crate) fn defaults<S: SecurityScheme>() -> Vec<&'static str> {
    S::scopes().iter().map(Scope::name).collect()
}

/// The scopes a guard demanding `required` authorizes scheme `S` for: `S`'s
/// defaults, then each of `required` that the defaults do not already name.
///
/// `required` joins the defaults rather than replacing them, so the scheme's
/// demand holds under every guard. Borrowed, and so free, when `S` has none.
fn demanded<S: SecurityScheme>(required: &'static [S::Scope]) -> Cow<'static, [S::Scope]> {
    let defaults = S::scopes();
    if defaults.is_empty() {
        return Cow::Borrowed(required);
    }

    let mut union = defaults.to_vec();
    for scope in required {
        if !union.iter().any(|held| held.name() == scope.name()) {
            union.push(scope.clone());
        }
    }
    Cow::Owned(union)
}

/// The names of the scopes [`demanded`] authorizes, without naming one twice.
fn demanded_names<S: SecurityScheme>(required: &'static [S::Scope]) -> Vec<&'static str> {
    let mut names = Vec::new();
    for name in demanded::<S>(required).iter().map(Scope::name) {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The component key scheme `S` is both registered and required under.
///
/// [`SecurityScheme::NAME`] need not be a legal component key and `Describe`
/// cannot report an error, so it is sanitized. An empty name falls back to the
/// scheme's type name.
pub(crate) fn component_name<S: SecurityScheme>() -> ComponentName {
    ComponentName::sanitized(S::NAME).unwrap_or_else(|_| {
        ComponentName::sanitized(std::any::type_name::<S>())
            .expect("a Rust type name is never empty")
    })
}
