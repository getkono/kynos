struct NotATag;

#[kynos::get("/users", tag = NotATag)]
async fn list() {}

fn main() {}
