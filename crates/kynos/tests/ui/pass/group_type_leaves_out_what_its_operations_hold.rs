//! A group's type names what the group applies, never what its operations
//! hold.
//!
//! An application's group function returns `Group<App>` whatever limits its
//! operations carry, so adding, removing or changing one operation's
//! interceptor inside `routes!` changes no signature a caller depends on. The
//! group's own interceptors stay in the type, which is what lets a later
//! `intercept` on it, and the router it is mounted into, be checked at compile
//! time.

use kynos::{
    middleware::{catch_panic::Propagate, limits::body_size::BodySize, request_id::RequestId},
    prelude::*,
};

struct App;

#[kynos::post("/imports")]
async fn create_import() {}

#[kynos::post("/backups")]
async fn restore_backup() {}

#[kynos::get("/imports")]
async fn list_imports() {}

fn imports(max_bytes: u64) -> Group<App> {
    Group::new("/").mount(kynos::routes![
        create_import.intercept(BodySize::new(max_bytes)),
        restore_backup.intercept(BodySize::new(max_bytes)),
        list_imports,
    ])
}

fn traced() -> Group<App, Propagate, kynos::stack![RequestId]> {
    Group::new("/traced")
        .intercept(RequestId::new())
        .mount(kynos::routes![create_import.intercept(BodySize::new(1024))])
}

fn main() {
    let _ = Router::<App>::new()
        .group(imports(1 << 20))
        .group(traced());
}
