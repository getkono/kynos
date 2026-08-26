//! A second tag would overwrite the first, and a silently discarded argument
//! is the defect the attribute's constants exist to prevent.

#[derive(kynos::Tag)]
struct Users;

#[derive(kynos::Tag)]
struct Admin;

#[kynos::get("/users", tag = Users, tag = Admin)]
async fn list() {}

fn main() {}
