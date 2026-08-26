// `method` belongs to `#[kynos::operation]` alone. Accepting it here would
// serve one method while the description named another.
#[kynos::get("/documents", method = "POST")]
async fn list_documents() {}

fn main() {}
