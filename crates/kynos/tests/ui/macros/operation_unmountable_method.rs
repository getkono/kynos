// The router mounts only the methods with a Path Item field, so a method
// outside them is refused here instead of panicking when `routes!` builds the
// endpoint. Control: `pass/operation_with_method.rs`, which mounts `GET` and,
// under `openapi32`, `QUERY`.
#[kynos::operation(method = "PROPFIND", path = "/files")]
async fn propfind() {}

fn main() {}
