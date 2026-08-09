//! The wrapper a handler receives a resolved dependency in.

use crate::di::{FromContext, Provides};

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

impl<C, T> FromContext<C> for Inject<T>
where
    C: Provides<T> + Sync,
    T: Send,
{
    async fn from_context(context: &C) -> Self {
        let _ = context;
        todo!()
    }
}
