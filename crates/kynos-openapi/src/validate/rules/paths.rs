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
        violation::{SpecError, Violation},
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
            let location = format!("#/paths/{raw}");

            let Ok(template) = PathTemplate::parse(raw.clone()) else {
                // An unparseable key cannot be checked further, and the parse
                // error itself is reported by whoever constructed it.
                continue;
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

            check_parameter_list(&location, &item.parameters, violations);

            for (method, operation) in item.operations() {
                let op_location = format!("{location}/{}", method.as_wire_str().to_lowercase());
                self.check_operation(
                    &op_location,
                    &template,
                    item,
                    operation,
                    &declared_schemes,
                    &declared_tags,
                    &mut operation_ids,
                    violations,
                );
            }
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
    let declared: HashSet<&str> = item
        .parameters
        .iter()
        .chain(operation.parameters.iter())
        .filter_map(RefOr::as_item)
        .filter(|parameter| parameter.location == ParameterIn::Path)
        .map(|parameter| parameter.name.as_str())
        .collect();

    for variable in template.variables() {
        if !declared.contains(variable.as_str()) {
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
