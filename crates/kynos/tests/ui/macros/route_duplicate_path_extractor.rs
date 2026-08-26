#[derive(kynos::PathParams)]
struct UserPath {
    id: u32,
}

// `Path` is spelled out rather than imported: the attribute bails before it
// emits the handler, so an import would be unused and put a warning in the
// snapshot alongside the diagnostic this case is about.
#[kynos::get("/users/{id}")]
async fn get_user(
    kynos::extract::params::path::Path(first): kynos::extract::params::path::Path<UserPath>,
    kynos::extract::params::path::Path(second): kynos::extract::params::path::Path<UserPath>,
) {
    let _ = (first, second);
}

fn main() {}
