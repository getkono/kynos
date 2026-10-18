//! The control for `macros/route_repeated_arguments`: the same routes,
//! differing only in that each argument is written once.

#[kynos::get("/users")]
async fn list_users() {}

#[kynos::get("/orders", operation_id = "listOrders")]
async fn list_orders() {}

#[kynos::operation(method = "GET", path = "/documents")]
async fn list_documents() {}

fn main() {}
