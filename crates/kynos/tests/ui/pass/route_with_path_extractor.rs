use kynos::extract::params::path::Path;

#[derive(kynos::PathParams)]
struct UserPath {
    id: u32,
}

#[kynos::get("/users/{id}")]
async fn get_user(Path(path): Path<UserPath>) {
    let _ = path;
}

fn main() {}
