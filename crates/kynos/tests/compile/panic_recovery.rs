//! Compile-only coverage for the panic-strategy assertion.

use kynos::Router;

fn main() {
    // Keep the call reachable to the compiler, but never execute a skeleton
    // method whose body is deliberately still `todo!()`.
    if std::hint::black_box(false) {
        let _router = Router::<()>::new().catch_panics();
    }
}
