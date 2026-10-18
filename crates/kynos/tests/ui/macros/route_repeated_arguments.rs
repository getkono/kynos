//! A route attribute reads each argument once: a second `path`,
//! `operation_id` or `method` silently replaced the first, so the route served
//! and described something other than what was written first.

#[kynos::get("/users", path = "/people")]
async fn list_users() {}

#[kynos::get("/orders", operation_id = "listOrders", operation_id = "orders")]
async fn list_orders() {}

#[kynos::operation(method = "GET", path = "/documents", method = "POST")]
async fn list_documents() {}

fn main() {}
