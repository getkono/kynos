//! Filing each interceptor header an operation placed on a response it holds
//! as a `$ref`, once the document declares what the reference names.
//!
//! Called by `describe.rs` after the application's `map_openapi` edits, since
//! `Registry` declares no response component and the referent exists only
//! once they ran; and before validation and hoisting, so both read the
//! header where it is filed.

use std::collections::{HashMap, HashSet};

use kynos_openapi::{Document, Header, Map, Method, Ref, RefOr, Response};
use serde_json::Value;

use super::operation::{OperationCx, Referred};

/// Where a response component's `$ref` points, before its name.
const RESPONSES: &str = "#/components/responses/";

/// Each header placed on a `$ref`, by the path and method of the operation
/// that placed it, in the order the router described them.
#[derive(Debug, Default)]
pub(super) struct Placed(Vec<(String, Method, Referred)>);

impl Placed {
    /// Finishes `cx`, the operation at `path` and `method`, holding what it
    /// placed on a `$ref`.
    pub(super) fn finish(
        &mut self,
        path: &str,
        method: Method,
        cx: OperationCx<'_>,
    ) -> kynos_openapi::Operation {
        let (operation, referred) = cx.finish_referred();
        self.0.extend(
            referred
                .into_iter()
                .map(|referred| (path.to_owned(), method, referred)),
        );
        operation
    }

    /// Files each held header on the response component its `$ref` names.
    ///
    /// A header every reference to the component carries alike is declared
    /// on the component, under the application's name for it. Otherwise each
    /// covered reference is replaced by a copy of the component, its own
    /// `description` and `summary` kept, declaring the header, and the
    /// component stays as written for the rest. A `HEAD`'s copy carries no
    /// content.
    ///
    /// Left alone: a reference an edit rewrote or moved, one naming no
    /// response component the document declares in place, a component a
    /// `Problems` placement does not reach, and a header the component
    /// already declares under that name. The first header placed under a
    /// name on one reference wins, as it does on an inline response.
    ///
    /// The `HEAD` the `GET` at each path in `heads` describes holds what that
    /// `GET` placed: it is the `GET`, served by the same chain, so it refers
    /// where the `GET` does.
    pub(super) fn file(self, document: &mut Document, heads: &HashSet<String>) {
        let derived = self
            .0
            .iter()
            .filter(|(path, method, _)| *method == Method::Get && heads.contains(path))
            .map(|(path, _, referred)| (path.clone(), Method::Head, referred.clone()))
            .collect::<Vec<_>>();
        // Each component reached, in the order first met.
        let mut reached: Map<Vec<Covered>> = Map::new();
        for (path, method, referred) in self.0.into_iter().chain(derived) {
            let Some(reference) = reference_at(document, &path, method, &referred.status)
                .filter(|reference| reference.location == referred.location)
            else {
                continue;
            };
            let Some(name) = referred.location.strip_prefix(RESPONSES) else {
                continue;
            };
            let Some(RefOr::Item(target)) = document.components.responses.get(name) else {
                continue;
            };
            if !referred.reaches(target) || target.headers.contains_key(&referred.name) {
                continue;
            }
            let reference = reference.clone();
            let referrers = reached.entry(name.to_owned()).or_default();
            let index = referrers
                .iter()
                .position(|covered| {
                    covered.path == path
                        && covered.method == method
                        && covered.status == referred.status
                })
                .unwrap_or_else(|| {
                    referrers.push(Covered {
                        path,
                        method,
                        status: referred.status,
                        reference,
                        headers: Map::new(),
                    });
                    referrers.len() - 1
                });
            referrers[index]
                .headers
                .entry(referred.name)
                .or_insert(referred.header);
        }
        if reached.is_empty() {
            return;
        }

        let references = references(document);
        for (name, covered) in reached {
            let all = references
                .get(name.as_str())
                .is_some_and(|count| *count == covered.len());
            let Some(RefOr::Item(target)) = document.components.responses.get_mut(&name) else {
                continue;
            };
            if all {
                file_shared(target, &covered);
            }
            let target = target.clone();
            for covered in covered {
                let mut missing = covered
                    .headers
                    .into_iter()
                    .filter(|(header, _)| !target.headers.contains_key(header))
                    .peekable();
                if missing.peek().is_none() {
                    continue;
                }
                let mut copy = in_place(&covered.reference, &target);
                // A `HEAD` is answered without content (RFC 9110 section
                // 9.3.2), as `derived_head` describes its inline responses.
                if covered.method == Method::Head {
                    copy.content.clear();
                }
                for (header, value) in missing {
                    copy.headers
                        .entry(header)
                        .or_insert_with(|| RefOr::Item(value));
                }
                if let Some(entry) =
                    entry_mut(document, &covered.path, covered.method, &covered.status)
                {
                    *entry = RefOr::Item(copy);
                }
            }
        }
    }
}

/// One reference to a reached component, and the headers placed on it.
#[derive(Debug)]
struct Covered {
    path: String,
    method: Method,
    status: String,
    reference: Ref,
    headers: Map<Header>,
}

/// Declares on `target` each header every one of `covered` carries alike, in
/// the order the first of them was placed.
fn file_shared(target: &mut Response, covered: &[Covered]) {
    let Some((first, rest)) = covered.split_first() else {
        return;
    };
    for (name, header) in &first.headers {
        if rest
            .iter()
            .all(|other| other.headers.get(name) == Some(header))
        {
            target
                .headers
                .entry(name.clone())
                .or_insert_with(|| RefOr::Item(header.clone()));
        }
    }
}

/// The response `reference` names, `target`, written in its place: the
/// reference's own `description`, and under `openapi32` its `summary`,
/// override the target's.
pub(super) fn in_place(reference: &Ref, target: &Response) -> Response {
    let mut inlined = target.clone();
    if let Some(description) = &reference.description {
        inlined.description = Some(description.clone());
    }
    #[cfg(feature = "openapi32")]
    if let Some(summary) = &reference.summary {
        inlined.summary = Some(summary.clone());
    }
    inlined
}

/// The `$ref` the operation at `path` and `method` declares under `status`.
fn reference_at<'d>(
    document: &'d Document,
    path: &str,
    method: Method,
    status: &str,
) -> Option<&'d Ref> {
    document
        .paths
        .items
        .get(path)?
        .operation(method)?
        .responses
        .responses
        .get(status)?
        .as_ref_object()
}

/// The response the operation at `path` and `method` declares under `status`.
fn entry_mut<'d>(
    document: &'d mut Document,
    path: &str,
    method: Method,
    status: &str,
) -> Option<&'d mut RefOr<Response>> {
    document
        .paths
        .items
        .get_mut(path)?
        .operation_mut(method)?
        .responses
        .responses
        .get_mut(status)
}

/// How many times the document writes a `$ref` to each response component,
/// by its name, wherever it is written: an operation's, a webhook's, a
/// callback's or another component's. Empty where the document does not
/// serialize, so no component is then written for every reference.
fn references(document: &Document) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    if let Ok(value) = serde_json::to_value(document) {
        count(&value, &mut counts);
    }
    counts
}

fn count(value: &Value, counts: &mut HashMap<String, usize>) {
    match value {
        Value::Object(object) => {
            if let Some(name) = object
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|location| location.strip_prefix(RESPONSES))
            {
                *counts.entry(name.to_owned()).or_default() += 1;
            }
            for nested in object.values() {
                count(nested, counts);
            }
        }
        Value::Array(items) => {
            for item in items {
                count(item, counts);
            }
        }
        _ => {}
    }
}
