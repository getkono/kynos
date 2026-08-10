//! A set of operations sharing a prefix, a tag, and interceptors.

use crate::{
    middleware::{
        Interceptor,
        catch_panic::{Catch, PanicPolicy, Propagate},
    },
    router::{endpoint::set::IntoEndpoints, operation::Tag},
};

/// A set of operations sharing a path prefix, a tag, and interceptors.
///
/// This is the recommended unit of API structure: one group per resource. The
/// prefix becomes part of each path, the tag is applied to each operation, and
/// each interceptor's contribution is merged into each operation's description
/// — so attaching authentication to a group documents it on every operation
/// underneath, correctly, without anyone maintaining that by hand.
#[derive(Debug)]
pub struct Group<C, P = Propagate> {
    // See `Router`: the parameters name a shape, not this value's auto traits.
    _private: std::marker::PhantomData<fn() -> (C, P)>,
}

impl<C> Group<C, Propagate> {
    /// Creates a group mounted at `prefix`.
    #[must_use]
    pub fn new(prefix: &'static str) -> Self {
        let _ = prefix;
        todo!()
    }
}

impl<C, P: PanicPolicy> Group<C, P> {
    /// Converts panics from covered operations into documented 500 responses.
    ///
    /// The policy is carried in the group's type and resolved while its
    /// endpoints are mounted. No recovery branch is installed when this method
    /// is not called.
    ///
    /// # Compile-time requirement
    ///
    /// The final binary must use `panic = "unwind"`. Selecting this policy in
    /// a `panic = "abort"` build is a compile-time error.
    ///
    /// ```no_run
    /// let users = kynos::router::group::Group::<()>::new("/users").catch_panics();
    /// # let _ = users;
    /// ```
    #[must_use]
    pub fn catch_panics(self) -> Group<C, Catch> {
        const {
            assert!(
                cfg!(panic = "unwind"),
                "Kynos panic recovery requires `panic = \"unwind\"`; remove `catch_panics` or enable unwinding"
            );
        }
        todo!()
    }

    /// Tags every operation in this group.
    #[must_use]
    pub fn tag<T: Tag>(self) -> Self {
        todo!()
    }

    /// Applies an interceptor to every operation in this group.
    #[must_use]
    pub fn intercept<I: Interceptor<C>>(self, interceptor: I) -> Self
    where
        C: Sync + 'static,
    {
        let _ = interceptor;
        todo!()
    }

    /// Mounts operations into this group.
    #[must_use]
    pub fn mount<E: IntoEndpoints<C>>(self, endpoints: E) -> Self {
        let _ = endpoints;
        todo!()
    }
}
