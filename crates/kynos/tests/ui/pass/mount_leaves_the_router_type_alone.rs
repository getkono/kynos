//! Mounting interceptor-free operations does not change the router's type.
//!
//! Pass-only on purpose: it asserts the *absence* of a type change, which has
//! no compile-fail dual. The pass-control rule asks for a control per negative
//! rather than the converse, so this stands alone as
//! `pass/into_endpoints_collection.rs` does.
//!
//! What it holds is the collapse in `Flatten`: `routes![a]` carries `()` and
//! `routes![b, c]` carries `Both<(), ()>`, and both fold away rather than
//! accumulating. Without it, re-assignment and a conditional mount -- two
//! idioms this repository uses -- would stop compiling the moment a router
//! remembered what it had mounted.

use kynos::prelude::*;

#[kynos::get("/users")]
async fn list() {}

#[kynos::get("/orders")]
async fn orders() {}

#[kynos::get("/carts")]
async fn carts() {}

#[kynos::get("/items")]
async fn items() {}

fn main() {
    let mut router = Router::<()>::new();

    // Re-assignment, not shadowing: the type on the right has to be the type
    // on the left.
    router = router.mount(kynos::routes![list]);
    router = router.mount(kynos::routes![orders, carts]);

    // Both arms of a conditional have to agree, which is the property
    // `Router::docs` returns `Self` for.
    let router = if std::env::var("EXTRA").is_ok() {
        router.mount(kynos::routes![items])
    } else {
        router
    };

    let _ = router;
}
