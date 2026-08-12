//! A set of operations sharing a prefix, a tag, and interceptors.

use crate::{
    middleware::{
        Interceptor,
        catch_panic::{Catch, PanicPolicy, Propagate},
        stack::{CompatibleStack, CompatibleWith, Cons},
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
pub struct Group<C, P = Propagate, I = ()> {
    // See `Router`: the parameters name a shape, not this value's auto traits,
    // and `I` is the interceptors mounted here as a type-level list.
    // The lint is measuring the three parameters the type genuinely
    // has; factoring them into an alias would hide the shape rather
    // than simplify it.
    #[allow(clippy::type_complexity)]
    _private: std::marker::PhantomData<fn() -> (C, P, I)>,
}

impl<C> Group<C, Propagate, ()> {
    /// Creates a group mounted at `prefix`.
    #[must_use]
    pub fn new(prefix: &'static str) -> Self {
        let _ = prefix;
        todo!()
    }
}

impl<C, P: PanicPolicy, I> Group<C, P, I> {
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
    pub fn intercept<N: Interceptor<C>>(self, interceptor: N) -> Group<C, P, Cons<N, I>>
    where
        C: Sync + 'static,
        I: CompatibleWith<N, C>,
    {
        let () = <I as CompatibleWith<N, C>>::CHECK;
        let _ = interceptor;
        todo!()
    }

    /// Mounts operations into this group.
    #[must_use]
    pub fn mount<E: IntoEndpoints<C>>(self, endpoints: E) -> Self
    where
        E::Stacks: CompatibleStack<I, C>,
    {
        let () = <E::Stacks as CompatibleStack<I, C>>::CHECK;
        let _ = endpoints;
        todo!()
    }
}
