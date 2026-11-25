//! A document with every response it names once under `components/responses`
//! written back out at each operation, for the suites asserting what one
//! operation declares rather than how the document shares it.
//!
//! Included with `#[path]` rather than depended on, for the reason
//! [`support/mod.rs`](mod.rs) gives.

use kynos::openapi::{Document, PathItem, RefOr, Response};

/// `document`, with each response `$ref` replaced by the component it names;
/// one naming no component of the document is left as written.
pub(crate) fn inlined(mut document: Document) -> Document {
    let shared = document.clone();
    let resolve = |entry: &mut RefOr<Response>| {
        if let Ok(response) = shared.resolve(entry).cloned() {
            *entry = RefOr::Item(response);
        }
    };
    for operation in document
        .paths
        .items
        .values_mut()
        .flat_map(PathItem::operations_mut)
    {
        let responses = &mut operation.responses;
        responses.default_response.iter_mut().for_each(&resolve);
        responses.responses.values_mut().for_each(&resolve);
    }
    document
}
