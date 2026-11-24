//! The `HEAD` a `GET` describes beside itself, which
//! [`describe_head`](super::endpoint::builder::EndpointBuilder::describe_head)
//! asks for: what `describe.rs` adds to the `paths` for it, and what
//! `build.rs` serves it by.

use std::collections::HashSet;

use kynos_openapi::{Components, Method, Operation, Parameter, ParameterIn, RefOr, Response};

use super::install::{error_at, path_location};
use super::{Arc, Mounted, PathItem, Paths, RouterError, Violation, build, dispatch};
use crate::router::operation::OperationIdCase;

/// Each `HEAD` a `GET` describes, held until every declared operation is in
/// `paths`: its key, its `operationId` and the `GET`.
#[derive(Default)]
pub(super) struct Heads(Vec<(String, String, Operation)>);

impl Heads {
    /// Holds the `HEAD` `mounted` describes, if any, from `operation`, its
    /// finished description, named as a router spelling derived names in
    /// `case` publishes it; refuses one described by anything but a `GET`.
    pub(super) fn defer<C: 'static>(
        &mut self,
        mounted: &Mounted<C>,
        method: Method,
        operation: &Operation,
        case: OperationIdCase,
        violations: &mut Vec<Violation<RouterError>>,
    ) {
        let Some(head) = mounted.endpoint.described_head(case) else {
            return;
        };
        let path = mounted.path.as_str().to_owned();
        if method == Method::Get {
            self.0.push((path, head.into_owned(), operation.clone()));
        } else {
            let location = format!(
                "{}/{}",
                path_location(&path),
                method.as_wire_str().to_lowercase()
            );
            violations.push(error_at(
                location,
                RouterError::HeadWithoutGet { method, path },
            ));
        }
    }

    /// Adds each held `HEAD` to `paths`, once `components`, which a response
    /// may reference, are complete too.
    ///
    /// A path that declares its own `HEAD` keeps it, and is reported; a second
    /// `GET` describing one is already reported as a duplicate `GET`.
    pub(super) fn describe(
        self,
        paths: &mut Paths,
        components: &Components,
        violations: &mut Vec<Violation<RouterError>>,
    ) {
        let mut derived: HashSet<String> = HashSet::new();
        for (key, head, get) in self.0 {
            let operation = derived_head(get, &head, components);
            let item: &mut PathItem = paths.items.entry(key.clone()).or_default();
            if item.operation(Method::Head).is_some() {
                if !derived.contains(&key) {
                    violations.push(error_at(
                        format!("{}/head", path_location(&key)),
                        RouterError::HeadDescribedTwice { path: key },
                    ));
                }
                continue;
            }
            item.set_operation(Method::Head, operation);
            derived.insert(key);
        }
    }
}

/// Each operation `mounted` describes, by path and method: every one's own,
/// and the `HEAD` each `GET` that describes one does.
#[cfg(feature = "unchecked")]
pub(super) fn described_operations<C: 'static>(mounted: &[Mounted<C>]) -> Vec<(&str, Method)> {
    mounted
        .iter()
        .flat_map(|mounted| {
            let method = mounted.endpoint.method();
            let head = (method == Method::Get
                && mounted
                    .endpoint
                    .described_head(OperationIdCase::AsWritten)
                    .is_some())
            .then_some(Method::Head);
            std::iter::once(method)
                .chain(head)
                .map(|method| (mounted.path.as_str(), method))
        })
        .collect()
}

/// The `HEAD` operation a `GET` describes beside itself: `get`, as the router
/// finished it, under `operation_id` and with no response carrying content
/// (RFC 9110 section 9.3.2), every header kept.
///
/// A response referenced from `components` whose target carries content is
/// inlined without it, keeping the reference's own overrides; any other
/// reference stays, since it names nothing to remove.
///
/// The `Range` and `If-Range` parameters and the 206 and 416 responses are
/// dropped: range handling is defined for `GET` alone (RFC 9110 section
/// 14.2), so the router ignores the fields on a `HEAD` and answers it with
/// neither status.
pub(super) fn derived_head(
    mut get: Operation,
    operation_id: &str,
    components: &Components,
) -> Operation {
    get.operation_id = Some(operation_id.to_owned());
    get.parameters
        .retain(|parameter| !is_range(parameter, components));
    let responses = &mut get.responses;
    for status in RANGE_STATUSES {
        responses.responses.shift_remove(status);
    }
    for response in responses
        .default_response
        .iter_mut()
        .chain(responses.responses.values_mut())
    {
        without_content(response, components);
    }
    get
}

/// The statuses only a ranged `GET` answers: 206 Partial Content and 416
/// Range Not Satisfiable (RFC 9110 sections 15.3.7 and 15.5.17).
const RANGE_STATUSES: [&str; 2] = ["206", "416"];

/// The request headers only a ranged `GET` reads: `Range`, and `If-Range`,
/// which a request without `Range` has ignored (RFC 9110 section 13.1.5).
const RANGE_FIELDS: [&str; 2] = ["range", "if-range"];

/// Whether `parameter`, or the one in `components` it references, is one of
/// [`RANGE_FIELDS`].
fn is_range(parameter: &RefOr<Parameter>, components: &Components) -> bool {
    let parameter = match parameter {
        RefOr::Item(parameter) => Some(parameter),
        RefOr::Ref(reference) => reference
            .location
            .strip_prefix("#/components/parameters/")
            .and_then(|name| components.parameters.get(name))
            .and_then(RefOr::as_item),
    };
    parameter.is_some_and(|parameter| {
        parameter.location == ParameterIn::Header
            && RANGE_FIELDS
                .iter()
                .any(|field| parameter.name.eq_ignore_ascii_case(field))
    })
}

/// `response` with its content removed, inlining a carrying target.
fn without_content(response: &mut RefOr<Response>, components: &Components) {
    match response {
        RefOr::Item(item) => item.content.clear(),
        RefOr::Ref(reference) => {
            let Some(target) = reference
                .location
                .strip_prefix("#/components/responses/")
                .and_then(|name| components.responses.get(name))
                .and_then(RefOr::as_item)
                .filter(|target| !target.content.is_empty())
            else {
                return;
            };
            let mut inlined = target.clone();
            inlined.content.clear();
            if let Some(description) = &reference.description {
                inlined.description = Some(description.clone());
            }
            #[cfg(feature = "openapi32")]
            if let Some(summary) = &reference.summary {
                inlined.summary = Some(summary.clone());
            }
            *response = RefOr::Item(inlined);
        }
    }
}

/// `get`, followed by the `HEAD` it describes where `item` holds one: the
/// same terminal under the same chain, as the one a `GET` answers undescribed
/// runs, under its own name, and right after `get`, where `Allow` names it.
pub(super) fn with_head<C>(
    get: dispatch::Served<C>,
    item: Option<&PathItem>,
) -> impl Iterator<Item = dispatch::Served<C>> {
    // Only a `GET`'s: `describe` refused one on any other method.
    let head = item
        .and_then(|item| item.operation(Method::Head))
        .map(|head| dispatch::Served {
            method: Method::Head,
            operation_id: dispatch::intern(head.operation_id.as_deref().unwrap_or_default()),
            terminal: Arc::clone(&get.terminal),
            interceptors: get.interceptors.clone(),
            catch_panics: get.catch_panics,
            secured: build::declares_security(head),
            #[cfg(feature = "unchecked")]
            unchecked_layers: get.unchecked_layers.clone(),
        });
    std::iter::once(get).chain(head)
}
