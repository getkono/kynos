// The generic attribute reads `method` itself and then hands its whole
// argument list to the shared parser, so the parser has to tolerate seeing it.
//
// Run, not only compiled: `routes!` builds each endpoint from its method name
// and panics on one the router cannot mount, so mounting both operations here
// is the control for `macros/operation_unmountable_method.rs` and
// `macros/operation_lowercase_method.rs`. `QUERY` is the one name past the
// eight that the router mounts, and only under `openapi32`, which this suite's
// `--all-features` build enables.

use kynos::prelude::*;

#[kynos::operation(method = "GET", path = "/documents")]
async fn list_documents() {}

#[kynos::operation(method = "QUERY", path = "/documents")]
async fn search_documents() {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![list_documents, search_documents]);
}
