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

/// What an edit may not change about an operation.
struct Identity {
    operation_id: Option<String>,
    opaque: bool,
}

impl Identity {
    /// Whether `edited` is still this operation: the same `operationId`, and
    /// still opaque if it was. Flagging one opaque is the edit's to declare.
    fn kept_by(&self, edited: &Self) -> bool {
        self.operation_id == edited.operation_id && (!self.opaque || edited.opaque)
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
