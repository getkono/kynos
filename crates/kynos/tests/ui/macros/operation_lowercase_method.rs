// HTTP method names are case-sensitive (RFC 9110 §9.1), so `get` is not `GET`
// and the router has nothing to mount it under. Control:
// `pass/operation_with_method.rs`, which spells it `GET` and mounts it.
#[kynos::operation(method = "get", path = "/documents")]
async fn list_documents() {}

fn main() {}
