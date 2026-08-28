//! Operation-level rules: identifier uniqueness, declared responses, tags,
//! security, and the content of a request body and every response.

use std::collections::{HashMap, HashSet};

use crate::{
    model::{
        paths::{item::PathItem, operation::Operation, template::PathTemplate},
        reference::RefOr,
        response::Response,
    },
    validate::{
        Validator,
        rules::{
            content::check_media_type,
            extensions::check_extensions,
            parameters::{check_header_map, check_parameter_list},
            paths::check_path_correspondence,
        },
        violation::{SpecError, Violation, pointer_token},
    },
};

impl Validator {
    /// Whether `name` is a Security Scheme Object's URI rather than a
    /// component name.
    ///
    /// 3.2 admits both (`references/3.2.0.md:4685`); 3.1 admits only a
    /// component name, so this is always `false` there.
    ///
    /// A *bare* single-segment name is not read as a URI even though one would
    /// be a legal relative reference. 3.2 says a name matching a component
    /// name is a component name, and that referencing by single-segment
    /// relative URI is spelled `./foo` — so `Bearer` with nothing declared is
    /// the misspelling this rule exists to catch, and accepting it would leave
    /// the rule with nothing to reject.
    fn names_a_scheme_by_uri(self, name: &str) -> bool {
        self.version.supports_3_2()
            && (name.contains('/') || name.contains(':') || name.contains('#'))
    }

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

        if !operation.responses.declares_a_response() {
            violations.push(Violation::error(location, SpecError::NoResponses));
        }

        // 3.1 marks a response's `description` REQUIRED and 3.2 does not, so
        // the model holds an `Option` and this is where the requirement is
        // applied — against the version the document claims, rather than
        // against both at once.
        if !self.version.supports_3_2() {
            let mut require_description = |status: &str, response: &RefOr<Response>| {
                if response
                    .as_item()
                    .is_some_and(|response| response.description.is_none())
                {
                    violations.push(Violation::error(
                        format!("{location}/responses/{status}"),
                        SpecError::MissingResponseDescription,
                    ));
                }
            };

            for (status, response) in &operation.responses.responses {
                require_description(status, response);
            }
            if let Some(default) = &operation.responses.default_response {
                require_description("default", default);
            }
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
                if !declared_schemes.contains(name.as_str())
                    && !self.names_a_scheme_by_uri(name.as_str())
                {
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
                &format!(
                    "{location}/requestBody/content/{}",
                    pointer_token(media_type)
                ),
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
                &format!("{response_location}/content/{}", pointer_token(media_type)),
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
