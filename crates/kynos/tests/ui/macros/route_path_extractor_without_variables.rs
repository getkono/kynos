#[derive(kynos::PathParams)]
struct UserPath {
    id: u32,
}

// `Path` is spelled out rather than imported: the attribute bails before it
// emits the handler, so an import would be unused and put a warning in the
// snapshot alongside the diagnostic this case is about.
#[kynos::get("/users")]
async fn list(
    kynos::extract::params::path::Path(path): kynos::extract::params::path::Path<UserPath>,
) {
    let _ = path;
}

fn main() {}
