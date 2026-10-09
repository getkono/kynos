//! A `PathParams` group declaring the route's variables in the route's order.

use kynos::extract::params::path::Path;

#[derive(kynos::PathParams)]
struct MemberPath {
    tenant: String,
    id: u32,
}

#[kynos::get("/tenants/{tenant}/members/{id}")]
async fn get_member(Path(path): Path<MemberPath>) {
    let _ = path;
}

fn main() {}
