//! README anti-pattern 10: trailing-slash policy is one application-level
//! decision, so a `Group` cannot make its own.

use kynos::router::{group::Group, policy::TrailingSlashPolicy};

fn main() {
    let _ = Group::<()>::new("/v1").trailing_slashes(TrailingSlashPolicy::Redirect);
}
