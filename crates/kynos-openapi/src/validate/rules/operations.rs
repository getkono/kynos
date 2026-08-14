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
            parameters::{check_header_map, check_parameter_list},
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
        // A response link used to be checked here too: it names one of
        // `operationRef` and `operationId`, which holds by construction, so a
        // link carries nothing this function could reject. The same is true of
        // a header's shape, its examples and its style -- but not of its name,
        // which is a key in the map rather than anything the value's type can
        // reach, so that one is still a rule.
        check_header_map(
            &format!("{response_location}/headers"),
            &response.headers,
            violations,
        );
    }
}
