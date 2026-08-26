//! Compile-only coverage for the panic-strategy assertion.

use kynos::Router;

fn main() {
    let _router = Router::<()>::new().catch_panics();
}
