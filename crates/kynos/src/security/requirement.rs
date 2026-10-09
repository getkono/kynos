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
//! | [`AnyOf<(A, B)>`](AnyOf) | `[{A}, {B}]` | `A`, then `B`; the first that authenticates wins |
//! | [`AllOf<(A, B)>`](AllOf) | `[{A, B}]` | both, and both must authenticate |
//!
//! [`MaybeAuth`](crate::security::auth::MaybeAuth) prepends `{}` to any of the
//! three. A combinator takes schemes and not other combinators, so each shape
//! has exactly one spelling.

use std::{future::Future, marker::PhantomData};

use kynos_openapi::{ComponentName, SecurityRequirement};

use crate::{
    error::rejection::AuthRejection,
    http::Parts,
    router::operation::OperationCx,
    security::{Authenticates, Authenticator, SecurityScheme, carrier::Carries},
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

    /// The `WWW-Authenticate` challenge a 401 refusing this requirement sends.
    ///
    /// The first scheme's that has one, in declaration order: a 401 needs only
    /// one challenge a client can answer.
    fn challenge() -> Option<&'static str>;

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

impl<S: SecurityScheme> sealed::Sealed for S {}

impl<S: SecurityScheme> Requirement for S {
    type Credential = <S as SecurityScheme>::Credential;

    fn challenge() -> Option<&'static str> {
        <S as SecurityScheme>::challenge()
    }

    fn declare(operation: &mut OperationCx<'_>) -> Vec<SecurityRequirement> {
        vec![require::<S>(operation, S::scopes().to_vec())]
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

        context
            .authenticator()
            .authenticate(presented, context)
            .await
            .map(Some)
    }
}

/// Any one of a tuple of schemes, tried in order.
///
/// Declares one requirement per scheme, `[{A}, {B}]`, which is OpenAPI's
/// "either". The first scheme that authenticates wins; a scheme whose
/// credential is absent is skipped. When none authenticates, the first refusal
/// in declaration order is the answer, and when nothing was presented at all
/// the requirement is absent.
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
/// 401, because a partial set is not an anonymous request.
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

/// Implements [`Requirement`] and [`CheckedBy`] for one [`AnyOf`] arity.
macro_rules! any_of {
    ($either:ident; $($scheme:ident => $variant:ident),+) => {
        impl<$($scheme: SecurityScheme),+> sealed::Sealed for AnyOf<($($scheme,)+)> {}

        impl<$($scheme: SecurityScheme),+> Requirement for AnyOf<($($scheme,)+)> {
            type Credential = $either<$(<$scheme as SecurityScheme>::Credential),+>;

            fn challenge() -> Option<&'static str> {
                first_challenge(&[$(<$scheme as SecurityScheme>::challenge),+])
            }

            fn declare(operation: &mut OperationCx<'_>) -> Vec<SecurityRequirement> {
                // Two schemes registered under one key are one alternative.
                let mut alternatives = Vec::new();
                $(
                    let requirement = require::<$scheme>(operation, $scheme::scopes().to_vec());
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
    };
}

/// Implements [`Requirement`] and [`CheckedBy`] for one [`AllOf`] arity.
macro_rules! all_of {
    ($($scheme:ident),+) => {
        impl<$($scheme: SecurityScheme),+> sealed::Sealed for AllOf<($($scheme,)+)> {}

        impl<$($scheme: SecurityScheme),+> Requirement for AllOf<($($scheme,)+)> {
            type Credential = ($(<$scheme as SecurityScheme>::Credential,)+);

            fn challenge() -> Option<&'static str> {
                first_challenge(&[$(<$scheme as SecurityScheme>::challenge),+])
            }

            fn declare(operation: &mut OperationCx<'_>) -> Vec<SecurityRequirement> {
                let mut together = SecurityRequirement::anonymous();
                $(
                    together
                        .0
                        .extend(require::<$scheme>(operation, $scheme::scopes().to_vec()).0);
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
                $(
                    let $scheme = <C as Authenticates<$scheme>>::authenticator(context)
                        .authenticate($scheme, context)
                        .await?;
                )+
                Ok(Some(($($scheme,)+)))
            }
        }
    };
}

any_of!(Either2; A => First, B => Second);
any_of!(Either3; A => First, B => Second, D => Third);
any_of!(Either4; A => First, B => Second, D => Third, E => Fourth);

all_of!(A, B);
all_of!(A, B, D);
all_of!(A, B, D, E);

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
