//! Operation-level rules: identifier uniqueness, declared responses, tags,
//! security, and the content of a request body and every response.

use std::collections::{HashMap, HashSet};

use crate::{
    model::{
        paths::{item::PathItem, operation::Operation, template::PathTemplate},
        reference::RefOr,
    },
    validate::{
        Validator,
        rules::{
            content::check_media_type,
            extensions::check_extensions,
            parameters::{check_header, check_parameter_list},
            paths::check_path_correspondence,
        },
        violation::{SpecError, Violation},
    },
};

impl Validator {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::validate) fn check_operation<'doc>(
        self,
        location: &str,
        template: &PathTemplate,
        item: &PathItem,
        operation: &'doc Operation,
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

        if operation.responses.is_empty() {
            violations.push(Violation::error(location, SpecError::NoResponses));
        }

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
                if !declared_schemes.contains(name.as_str()) {
                    violations.push(Violation::error(
                        location,
                        SpecError::UnknownSecurityScheme { name: name.clone() },
                    ));
                }
            }
        }

        check_parameter_list(location, &operation.parameters, violations);
        check_path_correspondence(location, template, item, operation, violations);
        check_operation_content(location, operation, violations);

        check_extensions(location, &operation.extensions, violations);
    }
}

/// Checks the request body and every response of one operation.
pub(in crate::validate) fn check_operation_content(
    location: &str,
    operation: &Operation,
    violations: &mut Vec<Violation>,
) {
    if let Some(RefOr::Item(body)) = &operation.request_body {
        for (media_type, content) in &body.content {
            check_media_type(
                &format!("{location}/requestBody/{media_type}"),
                content,
                violations,
            );
        }
    }

    for (status, response) in &operation.responses.responses {
        let Some(response) = response.as_item() else {
            continue;
        };
        let response_location = format!("{location}/responses/{status}");
        for (media_type, content) in &response.content {
            check_media_type(
                &format!("{response_location}/content/{media_type}"),
                content,
                violations,
            );
        }
        for (name, header) in &response.headers {
            if let Some(header) = header.as_item() {
                check_header(
                    &format!("{response_location}/headers/{name}"),
                    name,
                    header,
                    violations,
                );
            }
        }
        for (name, link) in &response.links {
            if let Some(link) = link.as_item() {
                let set = usize::from(link.operation_ref.is_some())
                    + usize::from(link.operation_id.is_some());
                if set != 1 {
                    violations.push(Violation::error(
                        format!("{response_location}/links/{name}"),
                        SpecError::LinkTargetExclusivity,
                    ));
                }
            }
        }
    }
}
