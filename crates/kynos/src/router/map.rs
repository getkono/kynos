//! Running the application's [`map_openapi`](super::Router::map_openapi)
//! edits on an assembled description, and refusing one that changes which
//! operations it describes.
//!
//! Called by `describe.rs` between assembling the document and validating it.

use std::collections::{HashMap, HashSet};

use kynos_openapi::{Method, Paths};

use super::install::{error_at, path_location};
use super::{Document, DocumentEdit, RouterError, Violation};

/// Runs every edit in call order, reporting each operation whose identity one
/// changed.
pub(super) fn apply(
    edits: &[DocumentEdit],
    document: &mut Document,
    violations: &mut Vec<Violation<RouterError>>,
) {
    if edits.is_empty() {
        return;
    }

    let assembled = identities(&document.paths);
    for edit in edits {
        edit(document);
    }
    let edited: HashMap<(String, Method), Identity> = identities(&document.paths)
        .into_iter()
        .map(|(path, method, identity)| ((path, method), identity))
        .collect();

    let mut held = HashSet::new();
    for (path, method, identity) in assembled {
        let kept = edited
            .get(&(path.clone(), method))
            .is_some_and(|after| identity.kept_by(after));
        if !kept {
            violations.push(mapped_operation(&path, method));
        }
        held.insert((path, method));
    }

    // Added by an edit: nothing serves it. In document order.
    for (path, method, _) in identities(&document.paths) {
        if !held.contains(&(path.clone(), method)) {
            violations.push(mapped_operation(&path, method));
        }
    }
}

/// What an edit may not change about an operation: what `build` reads from
/// the document to serve it.
struct Identity {
    operation_id: Option<String>,
    opaque: bool,
    /// Whether it declares a security requirement, which keeps the response
    /// cache from sharing what the guard admitted.
    secured: bool,
    /// The `Accept-Query` its 415 sends, which names what its extractor takes.
    #[cfg(feature = "openapi32")]
    accept_query: Option<crate::http::HeaderValue>,
}

impl Identity {
    /// Whether `edited` is still this operation: the same `operationId`,
    /// security presence and `Accept-Query`, and still opaque if it was.
    /// Flagging one opaque is the edit's to declare.
    fn kept_by(&self, edited: &Self) -> bool {
        self.operation_id == edited.operation_id
            && (!self.opaque || edited.opaque)
            && self.secured == edited.secured
            && self.same_accept_query(edited)
    }

    #[cfg(feature = "openapi32")]
    fn same_accept_query(&self, edited: &Self) -> bool {
        self.accept_query == edited.accept_query
    }

    #[cfg(not(feature = "openapi32"))]
    #[expect(clippy::unused_self, reason = "the signature the `openapi32` one has")]
    fn same_accept_query(&self, _edited: &Self) -> bool {
        true
    }
}

/// Every operation in `paths`, in document order.
fn identities(paths: &Paths) -> Vec<(String, Method, Identity)> {
    paths
        .items
        .iter()
        .flat_map(|(path, item)| {
            item.operations().map(move |(method, operation)| {
                let identity = Identity {
                    operation_id: operation.operation_id.clone(),
                    opaque: kynos_openapi::Opaque::is_annotated(operation),
                    secured: super::build::declares_security(operation),
                    #[cfg(feature = "openapi32")]
                    accept_query: super::install::accept_query(method, operation),
                };
                (path.clone(), method, identity)
            })
        })
        .collect()
}

/// The violation for an edit changing the operation at `method` and `path`.
fn mapped_operation(path: &str, method: Method) -> Violation<RouterError> {
    error_at(
        format!(
            "{}/{}",
            path_location(path),
            method.as_wire_str().to_lowercase()
        ),
        RouterError::MappedOperation {
            method,
            path: path.to_owned(),
        },
    )
}
