//! Path-level rules: template uniqueness, and the correspondence between a
//! template's variables and its `in: path` parameters.

use std::collections::{HashMap, HashSet};

use crate::{
    model::{
        document::Document,
        parameter::ParameterIn,
        paths::{item::PathItem, operation::Operation, template::PathTemplate},
        reference::RefOr,
    },
    validate::{
        Validator,
        rules::parameters::check_parameter_list,
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

        for (raw, item) in &document.paths.0 {
            let location = format!("#/paths/{}", pointer_token(raw));

            let template = match PathTemplate::parse(raw.clone()) {
                Ok(template) => template,
                Err(error) => {
                    // Nobody constructed this one: a document read from disk
                    // reaches here without ever passing through `parse`, so
                    // this is the only place the key is ever checked.
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
                &declared_schemes,
                &declared_tags,
                &mut operation_ids,
                violations,
            );
        }

        // Every other container an operation can be described in. The
        // specification scopes `operationId` uniqueness to "all operations
        // described in the API", and each of these describes some — so a walk
        // that stopped at `paths` left every operation-level rule stopping
        // there too. `rules/opaque.rs` already visits the same four.
        //
        // No template: a webhook name and a callback expression are not path
        // templates, so the correspondence rule has nothing to compare and is
        // skipped rather than fabricated. Every other rule applies unchanged.
        for (name, item) in &document.webhooks {
            self.check_item(
                &format!("#/webhooks/{}", pointer_token(name)),
                None,
                item,
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
            for (expression, item) in &callback.0 {
                let Some(item) = item.as_item() else { continue };
                self.check_item(
                    &format!(
                        "#/components/callbacks/{}/{}",
                        pointer_token(name),
                        pointer_token(expression)
                    ),
                    None,
                    item,
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
    /// `template` is `None` wherever the item hangs off something that is not
    /// a path — a webhook, a reusable component, a callback expression — which
    /// is the only rule that distinguishes those positions from `paths`.
    #[allow(clippy::too_many_arguments)]
    fn check_item<'doc>(
        self,
        location: &str,
        template: Option<&PathTemplate>,
        item: &'doc PathItem,
        declared_schemes: &HashSet<&str>,
        declared_tags: &HashSet<&str>,
        operation_ids: &mut HashMap<&'doc str, String>,
        violations: &mut Vec<Violation>,
    ) {
        check_parameter_list(location, &item.parameters, violations);

        let named = item
            .operations()
            .map(|(method, operation)| (method.as_wire_str().to_lowercase(), operation));

        // 3.2 puts an operation under `additionalOperations` when no field of
        // its own exists for the method, and `operations()` is `Method::all()`
        // driven, so it never yields one. An operation written there is as
        // real as one written beside it.
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
            self.check_operation(
                &format!("{location}/{segment}"),
                template,
                item,
                operation,
                declared_schemes,
                declared_tags,
                operation_ids,
                violations,
            );
        }
    }
}

/// Checks that path template variables and `in: path` parameters agree.
///
/// Parameters hoisted onto the enclosing Path Item count towards the
/// correspondence, so a shared parameter does not have to be repeated on every
/// operation.
pub(in crate::validate) fn check_path_correspondence(
    location: &str,
    template: &PathTemplate,
    item: &PathItem,
    operation: &Operation,
    violations: &mut Vec<Violation>,
) {
    // Declaration order, not hash order: `validate` promises violations "most
    // structural first", and a caller diffing two runs of the same document
    // must not see them shuffle.
    let mut declared: Vec<&str> = Vec::new();
    for parameter in item
        .parameters
        .iter()
        .chain(operation.parameters.iter())
        .filter_map(RefOr::as_item)
        .filter(|parameter| parameter.location == ParameterIn::Path)
    {
        let name = parameter.name.as_str();
        if !declared.contains(&name) {
            declared.push(name);
        }
    }

    for variable in template.variables() {
        if !declared.contains(&variable.as_str()) {
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
