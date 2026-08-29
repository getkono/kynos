//! A group interceptor is checked against the endpoints the group already
//! holds.
//!
//! The same defect one scope down. `Group::mount` returned `Self`, so the
//! endpoint's stack was checked against the group's and then dropped, and a
//! `Group::intercept` written afterwards was compared against nothing.

use kynos::{middleware::request_id::RequestId, prelude::*};

#[kynos::get("/users")]
async fn list() {}

fn main() {
    let _ = Group::<()>::new("/a")
        .mount(kynos::routes![list].0.intercept(RequestId::new()))
        .intercept(RequestId::new());
}
