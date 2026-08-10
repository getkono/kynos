use kynos::router::{Router, policy::TrailingSlashPolicy};

fn main() {
    // The surface is `todo!()`-bodied, so this asserts that the call
    // typechecks without running it. See `docs/testing.md`.
    if std::hint::black_box(false) {
        let _ = Router::<()>::new().trailing_slashes(TrailingSlashPolicy::Redirect);
    }
}
