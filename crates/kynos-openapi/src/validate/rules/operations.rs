//! Operation-level rules: identifier uniqueness, declared responses, tags,
//! security, the content of a request body and every response, and the
//! operation each link names.

use std::collections::{HashMap, HashSet};

use crate::{
    Map,
    model::{
        callback::Callback,
        components::Components,
        document::Document,
        link::{Link, LinkTarget},
        paths::{item::PathItem, operation::Operation, template::PathTemplate},
        reference::RefOr,
        response::status::StatusPattern,
    },
    pointer::pointer_token,
    validate::{
        Validator,
        rules::{
            content::{check_request_body, check_response},
            document::check_server_list,
            extensions::check_extensions,
            parameters::check_parameter_list,
            paths::check_path_correspondence,
        },
        violation::{SpecError, Violation},
    },
};

impl Validator {
    /// Whether the security requirement key `name` corresponds to a security
    /// scheme, given the component names in `declared`.
    ///
    /// A declared component name takes precedence under either version. 3.2
    /// also admits a scheme's URI, read by the predicate `Document::emit`
    /// refuses 3.1 on; 3.1 admits only a component name. A same-document
    /// pointer into the Security Schemes resolves here, so it must land on a
    /// declared one; a URI into another document cannot be followed, and
    /// passes. A pointer name escaped with `~` or `%` passes undecoded, since
    /// no valid component name needs either.
    pub(in crate::validate) fn names_a_scheme(self, name: &str, declared: &HashSet<&str>) -> bool {
        if declared.contains(name) {
            return true;
        }
        if !self.version.supports_3_2() || !crate::emit::downgrade::names_a_scheme_by_uri(name) {
            return false;
        }
        name.strip_prefix("#/components/securitySchemes/")
            .filter(|component| !component.contains(['/', '~', '%']))
            .is_none_or(|component| declared.contains(component))
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::validate) fn check_operation<'doc>(
        self,
        location: &str,
        template: Option<&PathTemplate>,
        item: &PathItem,
        operation: &'doc Operation,
        components: &Components,
        declared_schemes: &HashSet<&str>,
        declared_tags: &HashSet<&str>,
        operation_ids: &mut HashMap<&'doc str, String>,
        violations: &mut Vec<Violation>,
    ) {
        if let Some(id) = operation.operation_id.as_deref() {
            if let Some(first) = operation_ids.insert(id, location.to_owned()) {
                violations.push(Violation::error(
                    location,
                    SpecError::DuplicateOperationId {
                        operation_id: id.to_owned(),
                        first,
                    },
                ));
            }
        }

        if !operation.responses.declares_a_response() {
            violations.push(Violation::error(location, SpecError::NoResponses));
        }

        // The reader's own grammar, so a key accepted here is one it reads
        // back as a status rather than refusing it or taking it for `default`
        // or an extension.
        for key in operation.responses.responses.keys() {
            if key.parse::<StatusPattern>().is_err() {
                violations.push(Violation::error(
                    format!("{location}/responses/{}", pointer_token(key)),
                    SpecError::InvalidResponseKey { key: key.clone() },
                ));
            }
        }

        // A response's `description`, REQUIRED by 3.1 alone, is checked with
        // the rest of what 3.1 cannot express, wherever the response sits.

        for tag in &operation.tags {
            if !declared_tags.contains(tag.as_str()) {
                violations.push(Violation::warning(
                    location,
                    SpecError::UndocumentedTag { name: tag.clone() },
                ));
            }
        }

        for requirement in operation.security.iter().flatten() {
            for name in requirement.0.keys() {
                if !self.names_a_scheme(name, declared_schemes) {
                    violations.push(Violation::error(
                        location,
                        SpecError::UnknownSecurityScheme { name: name.clone() },
                    ));
                }
            }
        }

        check_server_list(location, &operation.servers, violations);
        check_parameter_list(location, &operation.parameters, components, violations);
        #[cfg(feature = "openapi32")]
        super::parameters::check_querystring(
            location,
            &item.parameters,
            &operation.parameters,
            components,
            violations,
        );
        if let Some(template) = template {
            check_path_correspondence(location, template, item, operation, components, violations);
        }
        check_operation_content(location, operation, components, violations);

        check_extensions(location, &operation.extensions, violations);
    }
}

/// Every operation a link can name, found by its own walk so a link anywhere
/// is judged against all of them.
#[derive(Default)]
struct Targets<'doc> {
    /// Each declared `operationId`.
    ids: HashSet<&'doc str>,
    /// The pointer each operation is described at, with the operation.
    operations: Vec<(String, &'doc Operation)>,
    /// Pointers to a Path Item, Callback or callback Path Item given by
    /// reference: the walk stops there, so what lies past one is unknown.
    references: Vec<String>,
    /// Whether one of those references leaves this document, which may then
    /// describe any `operationId`.
    elsewhere: bool,
}

impl<'doc> Targets<'doc> {
    fn of(document: &'doc Document) -> Self {
        let mut targets = Self::default();
        for (raw, item) in &document.paths.items {
            targets.item(&format!("#/paths/{}", pointer_token(raw)), item);
        }
        for (name, item) in &document.webhooks {
            targets.item(&format!("#/webhooks/{}", pointer_token(name)), item);
        }
        for (name, item) in &document.components.path_items {
            targets.item(
                &format!("#/components/pathItems/{}", pointer_token(name)),
                item,
            );
        }
        for (name, callback) in &document.components.callbacks {
            targets.callback(
                &format!("#/components/callbacks/{}", pointer_token(name)),
                callback,
            );
        }
        targets
    }

    fn reference(&mut self, location: &str, uri: &str) {
        self.elsewhere |= !uri.starts_with('#');
        self.references.push(location.to_owned());
    }

    fn item(&mut self, location: &str, item: &'doc PathItem) {
        if let Some(uri) = &item.reference {
            self.reference(location, uri);
        }
        let named = item
            .operations()
            .map(|(method, operation)| (method.as_wire_str().to_lowercase(), operation));
        #[cfg(feature = "openapi32")]
        let named = named.chain(
            item.additional_operations
                .iter()
                .map(|(method, operation)| {
                    (
                        format!("additionalOperations/{}", pointer_token(method)),
                        &**operation,
                    )
                }),
        );
        for (segment, operation) in named {
            let location = format!("{location}/{segment}");
            if let Some(id) = &operation.operation_id {
                self.ids.insert(id);
            }
            for (name, callback) in &operation.callbacks {
                self.callback(
                    &format!("{location}/callbacks/{}", pointer_token(name)),
                    callback,
                );
            }
            self.operations.push((location, operation));
        }
    }

    fn callback(&mut self, location: &str, callback: &'doc RefOr<Callback>) {
        let callback = match callback {
            RefOr::Ref(reference) => {
                return self.reference(location, &reference.location);
            }
            RefOr::Item(callback) => callback,
        };
        for (expression, item) in &callback.items {
            let location = format!("{location}/{}", pointer_token(expression));
            match item {
                RefOr::Ref(reference) => self.reference(&location, &reference.location),
                RefOr::Item(item) => self.item(&location, item),
            }
        }
    }

    /// Whether `operation_ref`, a same-document reference, may point at an
    /// Operation Object: it lands on one, or passes through a reference.
    fn resolves(&self, operation_ref: &str) -> bool {
        let Some(pointer) = operation_ref
            .strip_prefix('#')
            .and_then(percent_decode)
            .map(|fragment| format!("#{fragment}"))
        else {
            return false;
        };
        self.operations.iter().any(|(at, _)| *at == pointer)
            || self.references.iter().any(|at| {
                pointer
                    .strip_prefix(at.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
            })
    }
}

/// Decodes a URI fragment's percent-encoded octets, or `None` where an escape
/// is malformed or the octets are not UTF-8.
fn percent_decode(fragment: &str) -> Option<String> {
    let mut octets = Vec::with_capacity(fragment.len());
    let mut rest = fragment.as_bytes();
    while let Some((&octet, tail)) = rest.split_first() {
        if octet == b'%' {
            let (hex, tail) = tail.split_at_checked(2)?;
            let hex = std::str::from_utf8(hex).ok()?;
            if !hex.bytes().all(|digit| digit.is_ascii_hexdigit()) {
                return None;
            }
            octets.push(u8::from_str_radix(hex, 16).ok()?);
            rest = tail;
        } else {
            octets.push(octet);
            rest = tail;
        }
    }
    String::from_utf8(octets).ok()
}

/// Checks that every link names an operation the description describes.
///
/// An `operationId` must be one some operation declares, and a same-document
/// `operationRef` must point at an Operation Object. A link is reported where
/// it is defined: inline under an operation's response, or as a reusable
/// response's or link component; one given by reference is judged at its
/// component.
pub(in crate::validate) fn check_link_targets(
    document: &Document,
    violations: &mut Vec<Violation>,
) {
    let targets = Targets::of(document);

    let mut check = |location: String, links: &Map<RefOr<Link>>| {
        for (name, link) in links {
            if let Some(link) = link.as_item() {
                check_link_target(
                    &format!("{location}/links/{}", pointer_token(name)),
                    link,
                    &targets,
                    violations,
                );
            }
        }
    };

    for (location, operation) in &targets.operations {
        if let Some(RefOr::Item(response)) = &operation.responses.default_response {
            check(format!("{location}/responses/default"), &response.links);
        }
        for (status, response) in &operation.responses.responses {
            if let Some(response) = response.as_item() {
                check(format!("{location}/responses/{status}"), &response.links);
            }
        }
    }
    for (name, response) in &document.components.responses {
        if let Some(response) = response.as_item() {
            check(
                format!("#/components/responses/{}", pointer_token(name)),
                &response.links,
            );
        }
    }
    check("#/components".to_owned(), &document.components.links);
}

fn check_link_target(
    location: &str,
    link: &Link,
    targets: &Targets<'_>,
    violations: &mut Vec<Violation>,
) {
    match link.target() {
        LinkTarget::Id(id) => {
            if !targets.elsewhere && !targets.ids.contains(id.as_str()) {
                violations.push(Violation::error(
                    location,
                    SpecError::UnknownLinkOperationId {
                        operation_id: id.clone(),
                    },
                ));
            }
        }
        LinkTarget::Uri(uri) => {
            if uri.starts_with('#') && !targets.resolves(uri) {
                violations.push(Violation::error(
                    location,
                    SpecError::UnresolvedLinkOperationRef {
                        operation_ref: uri.clone(),
                    },
                ));
            }
        }
    }
}

/// Checks the inline request body and every inline response of one operation,
/// `default` included.
///
/// A referenced one is checked where it is defined, by
/// [`check_component_content`](super::content::check_component_content).
pub(in crate::validate) fn check_operation_content(
    location: &str,
    operation: &Operation,
    components: &Components,
    violations: &mut Vec<Violation>,
) {
    if let Some(RefOr::Item(body)) = &operation.request_body {
        check_request_body(
            &format!("{location}/requestBody"),
            body,
            components,
            violations,
        );
    }

    if let Some(RefOr::Item(response)) = &operation.responses.default_response {
        check_response(
            &format!("{location}/responses/default"),
            response,
            components,
            violations,
        );
    }
    for (status, response) in &operation.responses.responses {
        if let Some(response) = response.as_item() {
            check_response(
                &format!("{location}/responses/{status}"),
                response,
                components,
                violations,
            );
        }
    }
}
