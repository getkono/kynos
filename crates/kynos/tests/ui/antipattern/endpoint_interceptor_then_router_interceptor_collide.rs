//! A router interceptor is checked against the endpoints already mounted.
//!
//! `routes!` expands to a tuple so an endpoint's own stack survives to the
//! mount site -- and `mount` then checked it against the router's and dropped
//! it, so an `intercept` written afterwards saw nothing. Both write
//! `x-request-id` on `/users`.

use kynos::{middleware::request_id::RequestId, prelude::*};

#[kynos::get("/users")]
async fn list() {}

fn main() {
    let _ = Router::<()>::new()
        .mount(kynos::routes![list].0.intercept(RequestId::new()))
        .intercept(RequestId::new());
}
