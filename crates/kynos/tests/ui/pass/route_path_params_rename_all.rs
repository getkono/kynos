//! The route attribute holds `NAMES` to the template, so a template naming the
//! `rename_all` form compiles only if the derive reads `rename_all`.

use kynos::extract::params::path::Path;

#[derive(serde::Deserialize, kynos::PathParams)]
#[serde(rename_all = "camelCase")]
struct UserPath {
    user_id: u32,
}

#[kynos::get("/users/{userId}")]
async fn get_user(Path(path): Path<UserPath>) {
    let _ = path;
}

fn main() {}
