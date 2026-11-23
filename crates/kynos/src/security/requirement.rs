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
//! three. A combinator takes [`Member`]s, never other combinators, so each
//! shape has exactly one spelling.
//!
//! [`Scoped`] demands its scope set of a
//! scheme, or of every alternative of an [`AnyOf`] whose schemes share one
//! [`SecurityScheme::Scope`]: [`Scopable`] is what admits the two. Written as
//! a combinator's member, `Scoped<S, R>` demands its set of that member alone:
//!
//! | Parameter | `security` |
//! | --- | --- |
//! | `AnyOf<(A, Scoped<B, R>)>` | `[{A}, {B: R}]` |
//! | `AllOf<(A, Scoped<B, R>)>` | `[{A, B: R}]` |

use std::{borrow::Cow, future::Future, marker::PhantomData};

use kynos_openapi::{ComponentName, Parameter, SecurityRequirement};

use crate::{
    error::rejection::AuthRejection,
    http::Parts,
    router::operation::OperationCx,
    schema::registry::Registry,
    security::{
        Authenticates, Authenticator, Authorizer, Head, Scope, SecurityScheme,
        auth::{Scoped, Scopes},
        carrier::{Carries, presented::Presented},
    },
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
    ///
    /// `'static` for the reason [`SecurityScheme::Credential`] is.
    type Credential: Send + 'static;

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

    /// The request headers the schemes named read beside their credentials:
    /// every [`SecurityScheme::reads`], in declaration order.
    ///
    /// An [`AnyOf`]'s are not required, since no one alternative is; an
    /// [`AllOf`]'s keep what each scheme declared, since every one is read.
    fn reads(registry: &mut Registry) -> Vec<Parameter>;
}

/// One scheme as an [`AnyOf`] or an [`AllOf`] names it: bare, demanding its
/// default scopes, or as [`Scoped<S, R>`](Scoped), demanding `R` of `S` alone.
///
/// `Scoped<S, R>` here is the member, not the guard: it scopes one alternative
/// of an `AnyOf`, or one scheme of an `AllOf`, and leaves the others as they
/// are written. `AnyOf<(Session, Scoped<ApiToken, Write>)>` declares
/// `[{Session: []}, {ApiToken: [write]}]` and authorizes each alternative for
/// its own set.
///
/// ```no_run
/// # use kynos::security::{Scope, auth::{Auth, Scoped, Scopes}, requirement::AnyOf};
/// # use kynos::security::schemes::{Basic, Bearer};
/// # #[derive(Clone, Copy)]
/// # enum Grant { Write }
/// # impl Scope for Grant {
/// #     fn name(&self) -> &'static str { "write" }
/// # }
/// struct Write;
///
/// impl Scopes for Write {
///     type Scope = Grant;
///     const REQUIRED: &'static [Grant] = &[Grant::Write];
/// }
///
/// /// Any password, or a token granted `write`.
/// async fn publish(caller: Auth<AnyOf<(Basic, Scoped<Bearer<String, Grant>, Write>)>>) {}
///
/// /// A token granted `write`, through the guard rather than a member.
/// async fn edit(caller: Scoped<Bearer<String, Grant>, Write>) {}
///
/// /// A password or a token, each granted `write`, through the guard.
/// async fn delete(caller: Scoped<AnyOf<(Basic<String, Grant>, Bearer<String, Grant>)>, Write>) {}
/// ```
///
/// Only inside a combinator is `Scoped` a member: it is not a [`Requirement`],
/// so `Auth<Scoped<S, R>>` does not compile where the guard `Scoped<S, R>` is
/// meant, and an `AnyOf` holding a scoped member is not [`Scopable`], so a set
/// is never demanded of one scheme from two places.
///
/// ```compile_fail,E0277
/// # use kynos::security::{Scope, auth::{Auth, Scoped, Scopes}, schemes::Bearer};
/// # #[derive(Clone, Copy)]
/// # enum Grant { Write }
/// # impl Scope for Grant {
/// #     fn name(&self) -> &'static str { "write" }
/// # }
/// # struct Write;
/// # impl Scopes for Write {
/// #     type Scope = Grant;
/// #     const REQUIRED: &'static [Grant] = &[Grant::Write];
/// # }
/// async fn publish(caller: Auth<Scoped<Bearer<String, Grant>, Write>>) {}
/// ```
///
/// ```compile_fail,E0277
/// # use kynos::security::{Scope, auth::{Scoped, Scopes}, requirement::AnyOf};
/// # use kynos::security::schemes::{Basic, Bearer};
/// # #[derive(Clone, Copy)]
/// # enum Grant { Write }
/// # impl Scope for Grant {
/// #     fn name(&self) -> &'static str { "write" }
/// # }
/// # struct Write;
/// # impl Scopes for Write {
/// #     type Scope = Grant;
/// #     const REQUIRED: &'static [Grant] = &[Grant::Write];
/// # }
/// async fn publish(
///     caller: Scoped<AnyOf<(Basic<String, Grant>, Scoped<Bearer<String, Grant>, Write>)>, Write>,
/// ) {
/// }
/// ```
///
/// Sealed, as [`Requirement`] is.
pub trait Member: sealed::Sealed + 'static {
    /// The scheme named, whose carrier reads the credential and whose
    /// authenticator verifies it.
    type Scheme: SecurityScheme;

    /// Whether this member's guard may answer 403: the scheme's
    /// [`SecurityScheme::FORBIDS`] when bare, and always when scoped, since a
    /// demanded scope set may be refused.
    const FORBIDS: bool;

    /// The scopes the scheme is authorized for once it authenticates: its
    /// default scopes when bare, `None` where it has none, so it is never
    /// authorized; when scoped, its defaults and then each of `R::REQUIRED`
    /// they do not name, authorized even when empty.
    fn scopes() -> Option<Cow<'static, [<Self::Scheme as SecurityScheme>::Scope]>>;

    /// The names of the scopes the requirement naming this member lists:
    /// those [`scopes`](Self::scopes) authorizes.
    fn scope_names() -> Vec<&'static str>;
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

/// A [`Requirement`] a [`Scoped`] guard can
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

    fn reads(registry: &mut Registry) -> Vec<Parameter> {
        <S as SecurityScheme>::reads(registry)
    }
}

impl<C, S> CheckedBy<C> for S
where
    S: Carries,
    C: Authenticates<S>,
{
    async fn check(parts: &Parts, context: &C) -> Result<Option<S::Credential>, AuthRejection> {
        check_member::<S, C>(parts, context).await
    }
}

impl<S: SecurityScheme> Member for S {
    type Scheme = S;

    const FORBIDS: bool = <S as SecurityScheme>::FORBIDS;

    fn scopes() -> Option<Cow<'static, [<S as SecurityScheme>::Scope]>> {
        owed_defaults::<S>().map(Cow::Borrowed)
    }

    fn scope_names() -> Vec<&'static str> {
        defaults::<S>()
    }
}

impl<S, R> sealed::Sealed for Scoped<S, R>
where
    S: SecurityScheme,
    R: Scopes<Scope = <S as SecurityScheme>::Scope>,
{
}

impl<S, R> Member for Scoped<S, R>
where
    S: SecurityScheme,
    R: Scopes<Scope = <S as SecurityScheme>::Scope>,
{
    type Scheme = S;

    const FORBIDS: bool = true;

    fn scopes() -> Option<Cow<'static, [<S as SecurityScheme>::Scope]>> {
        Some(demanded::<S>(R::REQUIRED))
    }

    fn scope_names() -> Vec<&'static str> {
        demanded_names::<S>(R::REQUIRED)
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
        present_and_verify::<S, C>(parts, context, || Some(demanded::<S>(required))).await
    }
}

/// Any one of a tuple of schemes, tried in order.
///
/// Declares one requirement per scheme, `[{A}, {B}]`, which is OpenAPI's
/// "either". The first scheme that authenticates, and is authorized for its
/// [`Member::scopes`], wins; a scheme whose credential is absent is skipped.
/// When none is satisfied, the first refusal in declaration order is the
/// answer, and when nothing was presented at all the requirement is absent.
///
/// Implemented for tuples of two to four [`Member`]s, so an alternative written
/// [`Scoped<S, R>`](Scoped) demands `R` of itself alone. The credential is an
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
pub struct AnyOf<T>(PhantomData<fn() -> T>);

/// Every one of a tuple of schemes, together.
///
/// Declares a single requirement naming all of them, `[{A, B}]`, which is
/// OpenAPI's "both". Every credential is read before any is verified: when none
/// was presented the requirement is absent, and when only some were it is a
/// 401, because a partial set is not an anonymous request. Every scheme is
/// authenticated before any is authorized for its [`Member::scopes`], so a
/// credential that fails to authenticate is a 401 whatever another scheme's
/// scopes would have answered.
///
/// Implemented for tuples of two to four [`Member`]s, so a scheme written
/// [`Scoped<S, R>`](Scoped) demands `R` of itself alone. The credential is the
/// tuple of theirs, in the same order.
///
/// ```no_run
/// # use kynos::security::{auth::Auth, requirement::AllOf, schemes::{Bearer, MutualTls}};
/// async fn transfer(Auth((token, certificate)): Auth<AllOf<(Bearer, MutualTls)>>) {
///     todo!()
/// }
/// ```
pub struct AllOf<T>(PhantomData<fn() -> T>);

phantom_traits!(AnyOf<T>, AllOf<T>);

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

/// Appends `requirement` to an [`AnyOf`]'s `alternatives` unless it is there:
/// two schemes registered under one key are one alternative.
fn alternative(alternatives: &mut Vec<SecurityRequirement>, requirement: SecurityRequirement) {
    if !alternatives.contains(&requirement) {
        alternatives.push(requirement);
    }
}

/// `reads` with none required: what a guard declares of headers it reads only
/// where one alternative among several was presented.
pub(in crate::security) fn optional(mut reads: Vec<Parameter>) -> Vec<Parameter> {
    for parameter in &mut reads {
        parameter.required = None;
    }
    reads
}

/// One [`AnyOf`] alternative's outcome: its credential where it was satisfied,
/// and otherwise `None`, keeping the first refusal in `refused`. An absent
/// credential is skipped.
fn satisfied<T>(
    checked: Result<Option<T>, AuthRejection>,
    refused: &mut Option<AuthRejection>,
) -> Option<T> {
    checked.unwrap_or_else(|rejection| {
        refused.get_or_insert(rejection);
        None
    })
}

/// Implements [`Requirement`], [`CheckedBy`], [`Scopable`] and
/// [`ScopeCheckedBy`] for one [`AnyOf`] arity.
///
/// The first scheme is named apart, `$first`, so the others' scope types can
/// be bound to its.
macro_rules! any_of {
    ($either:ident; $first:ident; $($scheme:ident => $variant:ident),+; $($rest:ident),+) => {
        impl<$($scheme: Member),+> sealed::Sealed for AnyOf<($($scheme,)+)> {}

        impl<$($scheme: Member),+> Requirement for AnyOf<($($scheme,)+)> {
            type Credential = $either<$(CredentialOf<$scheme>),+>;

            const MAY_BE_UNAVAILABLE: bool =
                false $(|| <SchemeOf<$scheme> as SecurityScheme>::MAY_BE_UNAVAILABLE)+;

            const FORBIDS: bool = false $(|| <$scheme as Member>::FORBIDS)+;

            fn challenge() -> Option<&'static str> {
                first_challenge(&[$(<SchemeOf<$scheme> as SecurityScheme>::challenge),+])
            }

            fn insufficient_scope_challenge() -> Option<&'static str> {
                first_challenge(&[$(owed_scope_challenge::<$scheme>),+])
            }

            fn declare(operation: &mut OperationCx<'_>) -> Vec<SecurityRequirement> {
                let mut alternatives = Vec::new();
                $(
                    alternative(&mut alternatives, require_member::<$scheme>(operation));
                )+
                alternatives
            }

            fn reads(registry: &mut Registry) -> Vec<Parameter> {
                let mut reads = Vec::new();
                $( reads.extend(<SchemeOf<$scheme> as SecurityScheme>::reads(registry)); )+
                optional(reads)
            }
        }

        impl<C, $($scheme),+> CheckedBy<C> for AnyOf<($($scheme,)+)>
        where
            $($scheme: Member<Scheme: Carries>,)+
            $(C: Authenticates<SchemeOf<$scheme>>,)+
        {
            async fn check(
                parts: &Parts,
                context: &C,
            ) -> Result<Option<Self::Credential>, AuthRejection> {
                let mut refused = None;
                $(
                    let checked = check_member::<$scheme, C>(parts, context).await;
                    if let Some(credential) = satisfied(checked, &mut refused) {
                        return Ok(Some($either::$variant(credential)));
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
                let mut alternatives = Vec::new();
                $(
                    alternative(
                        &mut alternatives,
                        require::<$scheme>(operation, demanded_names::<$scheme>(required)),
                    );
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
                    let checked =
                        <$scheme as ScopeCheckedBy<C>>::check_scoped(parts, context, required)
                            .await;
                    if let Some(credential) = satisfied(checked, &mut refused) {
                        return Ok(Some($either::$variant(credential)));
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
        impl<$($scheme: Member),+> sealed::Sealed for AllOf<($($scheme,)+)> {}

        impl<$($scheme: Member),+> Requirement for AllOf<($($scheme,)+)> {
            type Credential = ($(CredentialOf<$scheme>,)+);

            const MAY_BE_UNAVAILABLE: bool =
                false $(|| <SchemeOf<$scheme> as SecurityScheme>::MAY_BE_UNAVAILABLE)+;

            const FORBIDS: bool = false $(|| <$scheme as Member>::FORBIDS)+;

            fn challenge() -> Option<&'static str> {
                first_challenge(&[$(<SchemeOf<$scheme> as SecurityScheme>::challenge),+])
            }

            fn insufficient_scope_challenge() -> Option<&'static str> {
                first_challenge(&[$(owed_scope_challenge::<$scheme>),+])
            }

            fn declare(operation: &mut OperationCx<'_>) -> Vec<SecurityRequirement> {
                let mut together = SecurityRequirement::anonymous();
                $( together.0.extend(require_member::<$scheme>(operation).0); )+
                vec![together]
            }

            fn reads(registry: &mut Registry) -> Vec<Parameter> {
                let mut reads = Vec::new();
                $( reads.extend(<SchemeOf<$scheme> as SecurityScheme>::reads(registry)); )+
                reads
            }
        }

        impl<C, $($scheme),+> CheckedBy<C> for AllOf<($($scheme,)+)>
        where
            $($scheme: Member<Scheme: Carries>,)+
            $(C: Authenticates<SchemeOf<$scheme>>,)+
        {
            #[allow(non_snake_case)]
            async fn check(
                parts: &Parts,
                context: &C,
            ) -> Result<Option<Self::Credential>, AuthRejection> {
                // Every carrier before any verifier, so a missing credential
                // costs no verification of the others.
                $( let $scheme = <SchemeOf<$scheme> as Carries>::present(parts); )+
                let absent = [$(is_absent::<SchemeOf<$scheme>, _>(&$scheme)),+];
                if absent.iter().all(|absent| *absent) {
                    return Ok(None);
                }
                if absent.iter().any(|absent| *absent) {
                    return Err(AuthRejection::unauthenticated());
                }
                // Every authentication before any authorization, so a request
                // one scheme cannot authenticate is a 401, never a 403, and is
                // never authorized.
                $(
                    let $scheme =
                        authenticate::<SchemeOf<$scheme>, C>($scheme, parts, context).await?;
                )+
                $(
                    let $scheme = authorize::<SchemeOf<$scheme>, C>(
                        $scheme,
                        <$scheme as Member>::scopes().as_deref(),
                        context,
                    )
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

/// Reads scheme `S`'s credential and, unless it is absence the guard answers,
/// verifies it for the `scopes` that are then computed.
async fn present_and_verify<S, C>(
    parts: &Parts,
    context: &C,
    scopes: impl FnOnce() -> Option<Cow<'static, [S::Scope]>>,
) -> Result<Option<S::Credential>, AuthRejection>
where
    S: Carries,
    C: Authenticates<S>,
{
    let presented = S::present(parts);
    if is_absent::<S, _>(&presented) {
        return Ok(None);
    }
    verify::<S, C>(presented, parts, scopes().as_deref(), context)
        .await
        .map(Some)
}

/// Whether `presented` is absence the guard answers: an
/// [`Absent`](Presented::Absent) credential of a scheme that does not hand
/// absence to its authenticator.
fn is_absent<S: Carries, T>(presented: &Presented<T>) -> bool {
    !S::OPTIONAL && matches!(presented, Presented::Absent)
}

/// Authenticates `presented` as scheme `S`, then authorizes it for `scopes`
/// where they are given.
///
/// `None` skips `authorize`, as a guard naming no scope set does for a scheme
/// with no default scopes ([`owed_defaults`]); `Some` calls it even with an
/// empty slice, as [`Scoped`] does.
async fn verify<S, C>(
    presented: Presented<S::Presented>,
    parts: &Parts,
    scopes: Option<&[S::Scope]>,
    context: &C,
) -> Result<S::Credential, AuthRejection>
where
    S: Carries,
    C: Authenticates<S>,
{
    let credential = authenticate::<S, C>(presented, parts, context).await?;
    authorize::<S, C>(credential, scopes, context).await
}

/// Authenticates `presented` as scheme `S`, beside the head of the request it
/// arrived on.
///
/// A 403 here declines a credential rather than a scope, so it sends no
/// challenge, whatever the authenticator set.
async fn authenticate<S, C>(
    presented: Presented<S::Presented>,
    parts: &Parts,
    context: &C,
) -> Result<S::Credential, AuthRejection>
where
    S: Carries,
    C: Authenticates<S>,
{
    context
        .authenticator()
        .authenticate(presented, Head::new(parts), context)
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

/// The challenge a guard naming no scope set may send on a 403 for member
/// `M`: its scheme's insufficient-scope challenge, where [`Member::scopes`]
/// authorizes it at all.
fn owed_scope_challenge<M: Member>() -> Option<&'static str> {
    M::scopes().and_then(|_| <M::Scheme as SecurityScheme>::insufficient_scope_challenge())
}

/// The scheme member `M` names.
type SchemeOf<M> = <M as Member>::Scheme;

/// What satisfying member `M` yields: its scheme's credential.
type CredentialOf<M> = <SchemeOf<M> as SecurityScheme>::Credential;

/// Reads member `M`'s credential and, unless it is absent, verifies it and
/// authorizes it for [`Member::scopes`].
async fn check_member<M, C>(
    parts: &Parts,
    context: &C,
) -> Result<Option<CredentialOf<M>>, AuthRejection>
where
    M: Member<Scheme: Carries>,
    C: Authenticates<M::Scheme>,
{
    present_and_verify::<M::Scheme, C>(parts, context, M::scopes).await
}

/// Registers member `M`'s scheme and returns the requirement naming it with
/// [`Member::scope_names`].
fn require_member<M: Member>(operation: &mut OperationCx<'_>) -> SecurityRequirement {
    require::<M::Scheme>(operation, M::scope_names())
}

/// Registers scheme `S` and returns the requirement naming it with `scopes`.
///
/// One name for both, so the requirement and the definition share a key.
fn require<S: SecurityScheme>(
    operation: &mut OperationCx<'_>,
    scopes: Vec<&'static str>,
) -> SecurityRequirement {
    let name = component_name::<S>();
    let requirement = SecurityRequirement::scoped(name.as_str(), scopes);
    operation.add_security_scheme(name, S::describe());
    requirement
}

/// The names of the scopes scheme `S` demands by default.
fn defaults<S: SecurityScheme>() -> Vec<&'static str> {
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
pub(in crate::security) fn component_name<S: SecurityScheme>() -> ComponentName {
    ComponentName::sanitized(S::NAME).unwrap_or_else(|_| {
        ComponentName::sanitized(std::any::type_name::<S>())
            .expect("a Rust type name is never empty")
    })
}
