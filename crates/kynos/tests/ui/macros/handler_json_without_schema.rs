//! A handler returning `Json<T>` whose `T` does not derive `Schema`.
//!
//! The newcomer's version of the mistake: written through the route attribute
//! and mounted through `routes!`, rather than through a bound spelled out by
//! hand. `ui/pass/handler_json_with_schema.rs` is the same program with the
//! derive.

use kynos::{extract::body::json::Json, router::endpoint::set::IntoEndpoints};

#[derive(serde::Serialize)]
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
