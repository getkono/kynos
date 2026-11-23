//! The collection a router mounts, and what can become one.

use std::{marker::PhantomData, sync::Arc};

use crate::{
    middleware::stack::{Both, Cons},
    router::endpoint::{DynEndpoint, Endpoint},
};

/// A set of operations waiting to be mounted.
///
/// What [`IntoEndpoints`] fills in. Opaque and append-only: the prefix, the
/// panic policy and the interceptors belong to whatever is mounting.
///
/// `S` is the interceptors its operations carry themselves, as a type-level
/// list the mount site checks, as it checks a `routes![..]` tuple's.
/// [`from_routes`](Endpoints::from_routes) is what fills it; a set built with
/// [`push`](Endpoints::push) carries `()`, whatever its members hold.
pub struct Endpoints<C, S = ()> {
    endpoints: Vec<Arc<dyn DynEndpoint<C>>>,
    _stacks: PhantomData<fn() -> S>,
}

impl<C, S> std::fmt::Debug for Endpoints<C, S> {
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
            _stacks: PhantomData,
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

    /// Collects `routes` — a `routes![..]` — into one value a function can
    /// name and return, so a module hands its operations out without a macro
    /// of its own.
    ///
    /// The interceptors its operations carry stay in the type, as the list
    /// `S`, and are checked wherever it is mounted, as the `routes![..]` would
    /// have been. Written with [`stack!`](crate::stack!), `S` names them in
    /// the order the `routes![..]` does, each operation's own in the order it
    /// mounts them, and is `()` where no operation carries one.
    ///
    /// ```no_run
    /// use kynos::{
    ///     extract::body::text::Text,
    ///     middleware::limits::body_size::BodySize,
    ///     response::status::NoContent,
    ///     router::{endpoint::set::Endpoints, group::Group},
    /// };
    ///
    /// #[kynos::get("/torrents")]
    /// async fn list_torrents() -> NoContent {
    ///     NoContent
    /// }
    ///
    /// #[kynos::post("/torrents")]
    /// async fn add_torrent(Text(metainfo): Text) -> NoContent {
    ///     let _ = metainfo;
    ///     NoContent
    /// }
    ///
    /// /// The torrent operations, the upload under a limit of its own.
    /// fn torrents() -> Endpoints<(), kynos::stack![BodySize]> {
    ///     Endpoints::from_routes(kynos::routes![
    ///         list_torrents,
    ///         add_torrent.intercept(BodySize::new(64 << 20)),
    ///     ])
    /// }
    ///
    /// let group = Group::<()>::new("/v1").mount(torrents());
    /// # let _ = group;
    /// ```
    ///
    /// The list is what the mount site checks, so an operation's interceptor
    /// colliding with its group's still fails to compile there:
    ///
    /// ```compile_fail,E0080
    /// # use kynos::{
    /// #     extract::body::text::Text,
    /// #     middleware::limits::body_size::BodySize,
    /// #     response::status::NoContent,
    /// #     router::{endpoint::set::Endpoints, group::Group},
    /// # };
    /// #[kynos::post("/torrents")]
    /// async fn add_torrent(Text(metainfo): Text) -> NoContent {
    ///     let _ = metainfo;
    ///     NoContent
    /// }
    ///
    /// fn torrents() -> Endpoints<(), kynos::stack![BodySize]> {
    ///     Endpoints::from_routes(kynos::routes![add_torrent.intercept(BodySize::new(64 << 20))])
    /// }
    ///
    /// let group = Group::<()>::new("/v1")
    ///     .intercept(BodySize::new(64 << 10))
    ///     .mount(torrents());
    /// # let _ = group;
    /// ```
    #[must_use]
    pub fn from_routes<E>(routes: E) -> Endpoints<C, <E::Stacks as Collect<()>>::Out>
    where
        E: IntoEndpoints<C>,
        E::Stacks: Collect<()>,
    {
        let mut collected = Self::new();
        routes.into_endpoints(&mut collected);
        Endpoints {
            endpoints: collected.endpoints,
            _stacks: PhantomData,
        }
    }
}

impl<C, S> Endpoints<C, S> {
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

    /// Hands the collected operations to whatever is mounting them.
    pub(crate) fn into_inner(self) -> Vec<Arc<dyn DynEndpoint<C>>> {
        self.endpoints
    }
}

/// A value that can contribute operations to a router or a group.
///
/// Implemented for [`Endpoints`], for [`EndpointBuilder`](crate::router::endpoint::builder::EndpointBuilder),
/// for an asset set, and for tuples, arrays and vectors of those. `routes![a, b, c]`
/// expands to a tuple, which is what lets it be one argument while each member
/// keeps the interceptor types the mount site checks.
///
/// There is no blanket implementation over [`Endpoint`], which would conflict
/// with the container implementations; a hand-written endpoint implements this
/// with `type Stacks = ();` and an `into_endpoints` of one line,
/// `sink.push(self)`.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not something a router can mount",
    label = "not mountable",
    note = "mount what `routes![..]` produces, an `EndpointBuilder`, or a tuple, array or vector \
            of those"
)]
pub trait IntoEndpoints<C> {
    /// The interceptors these operations carry, as a type-level list.
    ///
    /// `()` for anything already erased. Checked at the mount site against the
    /// router's own stack.
    type Stacks;

    /// Appends these operations to `sink`.
    fn into_endpoints(self, sink: &mut Endpoints<C>);
}

impl<C, S> IntoEndpoints<C> for Endpoints<C, S> {
    /// What [`from_routes`](Endpoints::from_routes) kept of its members'
    /// interceptors, or `()` for a set built with
    /// [`push`](Endpoints::push).
    type Stacks = S;

    fn into_endpoints(self, sink: &mut Endpoints<C>) {
        sink.endpoints.extend(self.endpoints);
    }
}

/// The interceptors a set of operations carries, gathered onto the list `S`
/// as one list: what [`Endpoints::from_routes`] keeps of a `routes![..]`.
///
/// Each member's list goes on in the order the members are written, so the
/// result reads, through [`stack!`](crate::stack!), in that order. Gathering
/// siblings into one list is sound for the reason
/// [`Flatten`](crate::middleware::stack::Flatten) gives: no check compares two
/// members of one list to each other.
pub trait Collect<S> {
    /// `S` with every interceptor here on top of it.
    type Out;
}

impl<S> Collect<S> for () {
    type Out = S;
}

impl<H, T: Collect<S>, S> Collect<S> for Cons<H, T> {
    type Out = Cons<H, T::Out>;
}

impl<L: Collect<S>, R: Collect<L::Out>, S> Collect<S> for Both<L, R> {
    type Out = R::Out;
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
            // `Both`, not a concatenation: no request reaches two operations.
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
