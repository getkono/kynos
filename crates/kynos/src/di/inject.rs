//! The wrapper a handler receives a resolved dependency in.

use core::convert::Infallible;

use crate::{
    di::Provides,
    extract::{FromRequestParts, describe::Describe},
    http::Parts,
    router::operation::OperationCx,
};

/// A dependency resolved from the application context.
///
/// ```no_run
/// # use kynos::di::inject::Inject;
/// # struct Db;
/// async fn list_users(Inject(db): Inject<Db>) {
///     todo!()
/// }
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Inject<T>(pub T);

impl<T> Inject<T> {
    /// Unwraps the injected value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

/// Injection cannot fail, so it contributes no response.
///
/// It is an ordinary extractor rather than a second kind of handler argument:
/// having one way for a value to reach a handler is what keeps the rule
/// "every argument describes itself" true without exceptions.
impl<C, T> FromRequestParts<C> for Inject<T>
where
    C: Provides<T> + Sync,
    T: Send,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        let _ = parts;
        Ok(Self(context.provide()))
    }
}

/// Application state is not part of the contract, so this contributes nothing.
///
/// A no-op here is a claim, not an omission: it says this argument is invisible
/// to a consumer. `MatchedPath` and `ConnectInfo` make the same claim.
impl<T> Describe for Inject<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
    }
}
