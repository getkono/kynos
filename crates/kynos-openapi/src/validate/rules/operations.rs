//! Operation-level rules: identifier uniqueness, declared responses, tags,
//! security, and the content of a request body and every response.

use std::collections::{HashMap, HashSet};

use crate::{
    model::{
        components::Components,
        paths::{item::PathItem, operation::Operation, template::PathTemplate},
        reference::RefOr,
        response::status::StatusPattern,
    },
    validate::{
        Validator,
        rules::{
            content::{check_request_body, check_response},
            extensions::check_extensions,
            parameters::check_parameter_list,
            paths::check_path_correspondence,
        },
        violation::{SpecError, Violation, pointer_token},
    },
};

impl Validator {
    /// Whether `name` is a Security Scheme Object's URI rather than a
    /// component name, by the predicate `Document::emit` refuses 3.1 on.
    ///
    /// 3.2 admits both; 3.1 admits only a component name, so this is always
    /// `false` there.
    pub(in crate::validate) fn names_a_scheme_by_uri(self, name: &str) -> bool {
        self.version.supports_3_2() && crate::emit::downgrade::names_a_scheme_by_uri(name)
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
        check_operation_content(location, operation, violations);

        check_extensions(location, &operation.extensions, violations);
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
    violations: &mut Vec<Violation>,
) {
    if let Some(RefOr::Item(body)) = &operation.request_body {
        check_request_body(&format!("{location}/requestBody"), body, violations);
    }

    if let Some(RefOr::Item(response)) = &operation.responses.default_response {
        check_response(
            &format!("{location}/responses/default"),
            response,
            violations,
        );
    }
    for (status, response) in &operation.responses.responses {
        if let Some(response) = response.as_item() {
            check_response(
                &format!("{location}/responses/{status}"),
                response,
                violations,
            );
        }
    }
}
