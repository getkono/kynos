//! The collection a router mounts, and what can become one.

use std::sync::Arc;

use crate::{
    middleware::stack::Both,
    router::endpoint::{DynEndpoint, Endpoint},
};

/// A set of operations waiting to be mounted.
///
/// What `routes![..]` produces, and what [`IntoEndpoints`] fills in. Opaque and
/// append-only: the prefix, the panic policy and the interceptors belong to
/// whatever is mounting, not to the endpoints, so there is nothing here for a
/// caller to reach into.
pub struct Endpoints<C> {
    endpoints: Vec<Arc<dyn DynEndpoint<C>>>,
}

impl<C> std::fmt::Debug for Endpoints<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoints")
            .field("len", &self.endpoints.len())
            .finish_non_exhaustive()
    }
}

impl<C> Default for Endpoints<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C> Endpoints<C> {
    /// Creates an empty set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            endpoints: Vec::new(),
        }
    }

    /// Adds one operation.
    pub fn push<E: Endpoint<C>>(&mut self, endpoint: E) -> &mut Self
    where
        C: Send + Sync + 'static,
    {
        self.endpoints.push(Arc::new(endpoint));
        self
    }

    /// The number of operations collected.
    #[must_use]
    pub fn len(&self) -> usize {
        self.endpoints.len()
    }

    /// Whether no operation has been collected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.endpoints.is_empty()
    }

    /// Moves every operation out of `other` into this set.
    pub(crate) fn absorb(&mut self, other: Self) {
        self.endpoints.extend(other.endpoints);
    }
}

/// A value that can contribute operations to a router or a group.
///
/// Implemented for [`Endpoints`], for [`EndpointBuilder`](crate::router::endpoint::builder::EndpointBuilder), and for tuples,
/// arrays and vectors of those — which is what lets `routes![a, b, c]` be one
/// argument.
///
/// There is deliberately no blanket implementation over [`Endpoint`]: it would
/// conflict with every one of the container implementations, because a
/// downstream crate may implement `Endpoint` for a tuple of its own types and
/// coherence has to assume it will. A hand-written endpoint is mounted with one
/// line — `sink.push(self)` — which is a small price for `routes!` working at
/// all.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not something a router can mount",
    label = "not mountable",
    note = "mount what `routes![..]` produces, an `EndpointBuilder`, or a tuple, array or vector \
            of those"
)]
pub trait IntoEndpoints<C> {
    /// The interceptors these operations carry, as a type-level list.
    ///
    /// `()` for anything already erased. `routes!` expands to a tuple rather
    /// than a collection precisely so this survives to the mount site, where it
    /// is checked against the router's own stack.
    type Stacks;

    /// Appends these operations to `sink`.
    fn into_endpoints(self, sink: &mut Endpoints<C>);
}

impl<C> IntoEndpoints<C> for Endpoints<C> {
    /// Already erased: an `Endpoints` cannot say what its members carry, which
    /// is why `routes!` does not build one.
    type Stacks = ();

    fn into_endpoints(self, sink: &mut Endpoints<C>) {
        sink.absorb(self);
    }
}

impl<C, T: IntoEndpoints<C>, const N: usize> IntoEndpoints<C> for [T; N] {
    type Stacks = T::Stacks;

    fn into_endpoints(self, sink: &mut Endpoints<C>) {
        for item in self {
            item.into_endpoints(sink);
        }
    }
}

impl<C, T: IntoEndpoints<C>> IntoEndpoints<C> for Vec<T> {
    type Stacks = T::Stacks;

    fn into_endpoints(self, sink: &mut Endpoints<C>) {
        for item in self {
            item.into_endpoints(sink);
        }
    }
}

/// Emits `IntoEndpoints` for one tuple arity.
macro_rules! tuple_endpoints {
    ($head:ident) => {
        impl<C, $head: IntoEndpoints<C>> IntoEndpoints<C> for ($head,) {
            type Stacks = $head::Stacks;

            #[allow(non_snake_case)]
            fn into_endpoints(self, sink: &mut Endpoints<C>) {
                let ($head,) = self;
                $head.into_endpoints(sink);
            }
        }
    };
    ($head:ident, $($tail:ident),+) => {
        impl<C, $head: IntoEndpoints<C>, $($tail: IntoEndpoints<C>),+> IntoEndpoints<C>
            for ($head, $($tail,)+)
        {
            // `Both` rather than a concatenation: two operations cannot collide
            // with each other, because no request reaches both. Only each one
            // against the router's own stack is worth checking.
            type Stacks = Both<$head::Stacks, <($($tail,)+) as IntoEndpoints<C>>::Stacks>;

            #[allow(non_snake_case)]
            fn into_endpoints(self, sink: &mut Endpoints<C>) {
                let ($head, $($tail,)+) = self;
                $head.into_endpoints(sink);
                $( $tail.into_endpoints(sink); )+
            }
        }
    };
}

tuple_endpoints!(A);
tuple_endpoints!(A, B);
tuple_endpoints!(A, B, C2);
tuple_endpoints!(A, B, C2, D);
tuple_endpoints!(A, B, C2, D, E);
tuple_endpoints!(A, B, C2, D, E, F);
tuple_endpoints!(A, B, C2, D, E, F, G);
tuple_endpoints!(A, B, C2, D, E, F, G, H);
tuple_endpoints!(A, B, C2, D, E, F, G, H, I);
tuple_endpoints!(A, B, C2, D, E, F, G, H, I, J);
tuple_endpoints!(A, B, C2, D, E, F, G, H, I, J, K);
tuple_endpoints!(A, B, C2, D, E, F, G, H, I, J, K, L);
tuple_endpoints!(A, B, C2, D, E, F, G, H, I, J, K, L, M);
tuple_endpoints!(A, B, C2, D, E, F, G, H, I, J, K, L, M, N);
tuple_endpoints!(A, B, C2, D, E, F, G, H, I, J, K, L, M, N, O);
tuple_endpoints!(A, B, C2, D, E, F, G, H, I, J, K, L, M, N, O, P);
