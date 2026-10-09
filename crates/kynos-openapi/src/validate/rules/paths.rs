//! Path-level rules: template uniqueness, and the correspondence between a
//! template's variables and its `in: path` parameters.

use std::collections::{HashMap, HashSet};

use crate::{
    model::{
        components::Components,
        document::Document,
        parameter::ParameterIn,
        paths::{item::PathItem, operation::Operation, template::PathTemplate},
    },
    validate::{
        Validator,
        rules::parameters::{Resolved, check_parameter_list, resolve_parameter},
        violation::{SpecError, Violation, pointer_token},
    },
};

impl Validator {
    pub(in crate::validate) fn check_paths(
        self,
        document: &Document,
        violations: &mut Vec<Violation>,
    ) {
        let declared_schemes: HashSet<&str> = document
            .components
            .security_schemes
            .keys()
            .map(String::as_str)
            .collect();
        let declared_tags: HashSet<&str> =
            document.tags.iter().map(|tag| tag.name.as_str()).collect();

        let mut operation_ids: HashMap<&str, String> = HashMap::new();
        let mut normalized_paths: HashMap<String, &String> = HashMap::new();

        for (raw, item) in &document.paths.items {
            let location = format!("#/paths/{}", pointer_token(raw));

            let template = match PathTemplate::parse(raw.clone()) {
                Ok(template) => template,
                Err(error) => {
                    // A deserialized key never passed through `parse`; this is
                    // the only place it is checked.
                    violations.push(Violation::error(
                        &location,
                        SpecError::InvalidPathTemplate {
                            template: raw.clone(),
                            reason: error,
                        },
                    ));
                    continue;
                }
            };

            if let Some(existing) = normalized_paths.insert(template.normalized(), raw) {
                if existing != raw {
                    violations.push(Violation::error(
                        &location,
                        SpecError::DuplicatePathTemplate {
                            template: raw.clone(),
                            existing: existing.clone(),
                        },
                    ));
                }
            }

            self.check_item(
                &location,
                Some(&template),
                item,
                &document.components,
                &declared_schemes,
                &declared_tags,
                &mut operation_ids,
                violations,
            );
        }

        // Every other container an operation can be described in, since
        // `operationId` is unique across "all operations described in the API".
        // No template: webhook names and callback expressions are not paths.
        for (name, item) in &document.webhooks {
            self.check_item(
                &format!("#/webhooks/{}", pointer_token(name)),
                None,
                item,
                &document.components,
                &declared_schemes,
                &declared_tags,
                &mut operation_ids,
                violations,
            );
        }

        for (name, item) in &document.components.path_items {
            self.check_item(
                &format!("#/components/pathItems/{}", pointer_token(name)),
                None,
                item,
                &document.components,
                &declared_schemes,
                &declared_tags,
                &mut operation_ids,
                violations,
            );
        }

        for (name, callback) in &document.components.callbacks {
            let Some(callback) = callback.as_item() else {
                continue;
            };
            for (expression, item) in &callback.items {
                let Some(item) = item.as_item() else { continue };
                self.check_item(
                    &format!(
                        "#/components/callbacks/{}/{}",
                        pointer_token(name),
                        pointer_token(expression)
                    ),
                    None,
                    item,
                    &document.components,
                    &declared_schemes,
                    &declared_tags,
                    &mut operation_ids,
                    violations,
                );
            }
        }
    }

    /// One Path Item's parameters and every operation on it.
    ///
    /// `template` is `None` where the item hangs off a webhook, a reusable
    /// component or a callback expression. Inline callbacks are walked too.
    #[allow(clippy::too_many_arguments)]
    fn check_item<'doc>(
        self,
        location: &str,
        template: Option<&PathTemplate>,
        item: &'doc PathItem,
        components: &Components,
        declared_schemes: &HashSet<&str>,
        declared_tags: &HashSet<&str>,
        operation_ids: &mut HashMap<&'doc str, String>,
        violations: &mut Vec<Violation>,
    ) {
        check_parameter_list(location, &item.parameters, components, violations);
        #[cfg(feature = "openapi32")]
        super::parameters::check_querystring(
            location,
            &[],
            &item.parameters,
            components,
            violations,
        );

        let named = item
            .operations()
            .map(|(method, operation)| (method.as_wire_str().to_lowercase(), operation));

        // `operations()` never yields an operation under 3.2's
        // `additionalOperations`.
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
            self.check_operation(
                &location,
                template,
                item,
                operation,
                components,
                declared_schemes,
                declared_tags,
                operation_ids,
                violations,
            );

            // Inline callbacks only, without a template; a referenced one is
            // checked at its component, which also bounds the recursion.
            for (name, callback) in &operation.callbacks {
                let Some(callback) = callback.as_item() else {
                    continue;
                };
                for (expression, item) in &callback.items {
                    let Some(item) = item.as_item() else { continue };
                    self.check_item(
                        &format!(
                            "{location}/callbacks/{}/{}",
                            pointer_token(name),
                            pointer_token(expression)
                        ),
                        None,
                        item,
                        components,
                        declared_schemes,
                        declared_tags,
                        operation_ids,
                        violations,
                    );
                }
            }
        }
    }
}

/// Checks that path template variables and `in: path` parameters agree.
///
/// Parameters on the enclosing Path Item count, and a `$ref` into
/// `#/components/parameters` counts as the parameter it names.
pub(in crate::validate) fn check_path_correspondence(
    location: &str,
    template: &PathTemplate,
    item: &PathItem,
    operation: &Operation,
    components: &Components,
    violations: &mut Vec<Violation>,
) {
    // Declaration order, not hash order, so violations are stable across runs.
    let mut declared: Vec<&str> = Vec::new();
    // An unfollowable reference may declare any variable, so none is reported
    // undeclared; a missing component declares nothing.
    let mut unknowable = false;
    for entry in item.parameters.iter().chain(operation.parameters.iter()) {
        let parameter = match resolve_parameter(entry, components) {
            Resolved::Found(parameter) => parameter,
            Resolved::Missing => continue,
            Resolved::Elsewhere => {
                unknowable = true;
                continue;
            }
        };
        if parameter.location != ParameterIn::Path {
            continue;
        }
        let name = parameter.name.as_str();
        if !declared.contains(&name) {
            declared.push(name);
        }
    }

    for variable in template.variables() {
        if !unknowable && !declared.contains(&variable.as_str()) {
            violations.push(Violation::error(
                location,
                SpecError::UndeclaredPathVariable {
                    name: variable.clone(),
                },
            ));
        }
    }
    for name in &declared {
        if !template.variables().iter().any(|v| v == name) {
            violations.push(Violation::error(
                location,
                SpecError::UnusedPathParameter {
                    name: (*name).to_owned(),
                },
            ));
        }
    }
}
