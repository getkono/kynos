//! A handler returning `Json<T>` whose `T` derives `Schema` is mounted.

use kynos::{extract::body::json::Json, router::endpoint::set::IntoEndpoints};

#[derive(serde::Serialize, kynos::Schema)]
struct User {
    id: u32,
}

#[kynos::get("/users/me")]
async fn me() -> Json<User> {
    Json(User { id: 1 })
}

fn main() {
    let mut sink = kynos::router::endpoint::set::Endpoints::<()>::new();
    kynos::routes![me].into_endpoints(&mut sink);
}
