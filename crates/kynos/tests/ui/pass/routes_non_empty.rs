//! `routes!` with at least one operation is accepted.
//!
//! It yields a tuple rather than an `Endpoints`, so that each operation's own
//! interceptors survive to the mount site; `IntoEndpoints` is what erases them,
//! once the check has run.

use kynos::router::endpoint::set::IntoEndpoints;

#[kynos::get("/users")]
async fn list() {}

fn main() {
    if std::hint::black_box(false) {
        let mut sink = kynos::router::endpoint::set::Endpoints::<()>::new();
        kynos::routes![list].into_endpoints(&mut sink);
    }
}
