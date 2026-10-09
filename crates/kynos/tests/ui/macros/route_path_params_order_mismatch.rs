//! A group declaring the route's variables out of order is refused, naming the
//! first parameter out of place and the variable the route has there. Its pass
//! sibling, `ui/pass/route_path_params_names_in_order.rs`, is in route order.

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
