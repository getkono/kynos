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
    pointer::At,
    validate::{
        Validator,
        rules::{
            document::check_server_list,
            parameters::{Resolved, check_parameter_list, resolve_parameter},
        },
        violation::{SpecError, Violation},
    },
    visit::{self, Visit},
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

        // Every container an operation can be described in, since
        // `operationId` is unique across "all operations described in the API".
        visit::document(
            document,
            &mut Items {
                validator: self,
                components: &document.components,
                declared_schemes,
                declared_tags,
                operation_ids: HashMap::new(),
                normalized_paths: HashMap::new(),
                template: None,
                violations,
            },
        );
    }
}

/// What the walk over every Path Item and operation reads and gathers.
struct Items<'doc, 'v> {
    validator: Validator,
    components: &'doc Components,
    declared_schemes: HashSet<&'doc str>,
    declared_tags: HashSet<&'doc str>,
    /// Where each `operationId` was first seen.
    operation_ids: HashMap<&'doc str, String>,
    /// The `paths` key each normalized template was first seen under.
    normalized_paths: HashMap<String, &'doc str>,
    /// The template of the `paths` entry being walked, with its Path Item.
    ///
    /// Only that item's own operations read it: webhook names and callback
    /// expressions, including those below it, are not paths.
    template: Option<(&'doc PathItem, PathTemplate)>,
    violations: &'v mut Vec<Violation>,
}

impl<'doc> Visit<'doc> for Items<'doc, '_> {
    /// One Path Item's template, where it has one, and parameters.
    ///
    /// A `paths` key that is no template skips the item: nothing below it can
    /// be judged against its variables.
    fn path_item(&mut self, at: At<'_>, item: &'doc PathItem, path: Option<&'doc str>) -> bool {
        let location = at.pointer();

        if let Some(raw) = path {
            let template = match PathTemplate::parse(raw) {
                Ok(template) => template,
                Err(error) => {
                    // A deserialized key never passed through `parse`; this is
                    // the only place it is checked.
                    self.violations.push(Violation::error(
                        &location,
                        SpecError::InvalidPathTemplate {
                            template: raw.to_owned(),
                            reason: error,
                        },
                    ));
                    return false;
                }
            };

            if let Some(existing) = self.normalized_paths.insert(template.normalized(), raw) {
                if existing != raw {
                    self.violations.push(Violation::error(
                        &location,
                        SpecError::DuplicatePathTemplate {
                            template: raw.to_owned(),
                            existing: existing.to_owned(),
                        },
                    ));
                }
            }

            self.template = Some((item, template));
        }

        check_server_list(&location, &item.servers, self.violations);
        check_parameter_list(
            &location,
            &item.parameters,
            self.components,
            self.violations,
        );
        #[cfg(feature = "openapi32")]
        super::parameters::check_querystring(
            &location,
            &[],
            &item.parameters,
            self.components,
            self.violations,
        );

        // `operations()` never yields an operation under 3.2's
        // `additionalOperations`. A key naming a fixed-field method is refused
        // at the key, and its operation is still checked like any other.
        #[cfg(feature = "openapi32")]
        for key in item.additional_operations.keys() {
            if let Some(method) = crate::model::paths::method::Method::from_wire_str(key) {
                self.violations.push(Violation::error(
                    format!(
                        "{location}/additionalOperations/{}",
                        crate::pointer::pointer_token(key)
                    ),
                    SpecError::FixedMethodInAdditionalOperations { method },
                ));
            }
        }

        true
    }

    fn operation(&mut self, at: At<'_>, item: &'doc PathItem, operation: &'doc Operation) {
        let template = self
            .template
            .as_ref()
            .filter(|(holder, _)| std::ptr::eq(*holder, item))
            .map(|(_, template)| template);
        self.validator.check_operation(
            &at.pointer(),
            template,
            item,
            operation,
            self.components,
            &self.declared_schemes,
            &self.declared_tags,
            &mut self.operation_ids,
            self.violations,
        );
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
