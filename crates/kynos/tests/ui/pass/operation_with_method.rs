// The generic attribute reads `method` itself and then hands its whole
// argument list to the shared parser, so the parser has to tolerate seeing it.
#[kynos::operation(method = "GET", path = "/documents")]
async fn list_documents() {}

fn main() {}
