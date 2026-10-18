//! A tuple of endpoints is mountable up to sixteen members; a seventeenth is
//! refused at compile time. Its control is
//! `ui/pass/into_endpoints_sixteen_members.rs`.

use kynos::router::endpoint::set::{Endpoints, IntoEndpoints};

fn mountable<C, T: IntoEndpoints<C>>() {}

type E = Endpoints<()>;

fn main() {
    mountable::<(), (E, E, E, E, E, E, E, E, E, E, E, E, E, E, E, E, E)>();
}
