//! A `PathParams` group declaring the route's variables out of order names the
//! variable each position should hold. `ui/pass/route_path_params_names_in_order.rs`
//! is the same program in the route's order.

use kynos::extract::params::path::Path;

#[derive(kynos::PathParams)]
struct MemberPath {
    id: u32,
    tenant: String,
}

#[kynos::get("/tenants/{tenant}/members/{id}")]
async fn get_member(Path(path): Path<MemberPath>) {
    let _ = path;
}

fn main() {}
