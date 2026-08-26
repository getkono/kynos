#[derive(kynos::Tag)]
struct Users;

#[kynos::get("/users", tag = Users)]
async fn list() {}

fn main() {}
