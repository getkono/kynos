//! The control for `ui/traits/into_endpoints_seventeen_members.rs`: a tuple of
//! sixteen endpoint sets, the most `IntoEndpoints` accepts, is mountable.

use kynos::router::endpoint::set::{Endpoints, IntoEndpoints};

fn mountable<C, T: IntoEndpoints<C>>() {}

type E = Endpoints<()>;

fn main() {
    mountable::<(), (E, E, E, E, E, E, E, E, E, E, E, E, E, E, E, E)>();
}
