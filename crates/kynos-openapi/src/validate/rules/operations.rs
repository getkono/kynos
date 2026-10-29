//! Operation-level rules: identifier uniqueness, declared responses, tags,
//! security, the content of a request body and every response, and the
//! operation each link names.

use std::collections::{HashSet, hash_map::Entry};

use crate::{
    Map,
    model::{
        components::Components,
        document::Document,
        link::{Link, LinkTarget},
        paths::{item::PathItem, operation::Operation},
        reference::RefOr,
        response::status::StatusPattern,
    },
    pointer::{At, pointer_token},
    validate::{
        rules::{
            content::{check_request_body, check_response},
            document::check_server_list,
            parameters::check_parameter_list,
            paths::{Items, check_path_correspondence},
        },
        violation::{SpecError, Violation},
    },
    visit::{self, Visit},
};

impl<'doc> Items<'doc, '_> {
    /// Checks one operation, held by `item`, at `at`.
    ///
    /// Path correspondence is checked only where `item` is the `paths` entry
    /// whose template is being walked.
    pub(in crate::validate) fn check_operation(
        &mut self,
        at: At<'_>,
        item: &PathItem,
        operation: &'doc Operation,
    ) {
        let Self {
            validator,
            components,
            declared_schemes,
            declared_tags,
            operation_ids,
            template,
            violations,
            ..
        } = self;
        let components: &Components = components;
        let template = template
            .as_ref()
            .filter(|(holder, _)| std::ptr::eq(*holder, item))
            .map(|(_, template)| template);
        let location = at.pointer();
        let location = location.as_str();

        if let Some(id) = operation.operation_id.as_deref() {
            match operation_ids.entry(id) {
                Entry::Occupied(first) => violations.push(Violation::error(
                    location,
                    SpecError::DuplicateOperationId {
                        operation_id: id.to_owned(),
                        first: first.get().clone(),
                    },
                )),
                Entry::Vacant(slot) => {
                    slot.insert(location.to_owned());
                }
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
                if !validator.names_a_scheme(name, declared_schemes) {
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
        check_operation_content(at, operation, components, violations);
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

impl<'doc> Visit<'doc> for Targets<'doc> {
    /// A Path Item's own `$ref` stops the walk as much as a referenced one.
    fn path_item(&mut self, at: At<'_>, item: &'doc PathItem, _path: Option<&'doc str>) -> bool {
        if let Some(uri) = &item.reference {
            self.reference(at, uri);
        }
        true
    }

    fn operation_walked(&mut self, at: At<'_>, operation: &'doc Operation) {
        if let Some(id) = &operation.operation_id {
            self.ids.insert(id);
        }
        self.operations.push((at.pointer(), operation));
    }

    fn reference(&mut self, at: At<'_>, uri: &'doc str) {
        self.elsewhere |= !uri.starts_with('#');
        self.references.push(at.pointer());
    }
}

impl<'doc> Targets<'doc> {
    fn of(document: &'doc Document) -> Self {
        let mut targets = Self::default();
        visit::document(document, &mut targets);
        targets
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
        for (key, response) in operation.responses.entries() {
            check(
                format!("{location}/responses/{}", pointer_token(key)),
                &response.links,
            );
        }
    }
    let components = At::ROOT.key("components");
    visit::inline_items(
        components.key("responses"),
        &document.components.responses,
        |at, response| check(at.pointer(), &response.links),
    );
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
fn check_operation_content(
    at: At<'_>,
    operation: &Operation,
    components: &Components,
    violations: &mut Vec<Violation>,
) {
    if let Some(RefOr::Item(body)) = &operation.request_body {
        check_request_body(at.key("requestBody"), body, components, violations);
    }

    let responses = at.key("responses");
    for (key, response) in operation.responses.entries() {
        check_response(responses.key(key), response, components, violations);
    }
}
