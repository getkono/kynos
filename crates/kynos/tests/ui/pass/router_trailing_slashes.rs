use kynos::router::{Router, policy::TrailingSlashPolicy};

fn main() {
    let _ = Router::<()>::new().trailing_slashes(TrailingSlashPolicy::Redirect);
}
