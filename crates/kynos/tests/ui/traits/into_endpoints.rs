//! A router mounts operations, and a bare integer is not a collection of them.

fn mountable<C, E: kynos::router::endpoint::set::IntoEndpoints<C>>() {}

fn main() {
    mountable::<(), u32>();
}
