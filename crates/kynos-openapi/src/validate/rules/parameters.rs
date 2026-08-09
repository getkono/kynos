//! Parameter and header rules: uniqueness, the schema/content exclusion, the
//! closed style table, and the headers the specification refuses to describe.

use std::collections::HashSet;

use crate::{
    model::{
        parameter::{
            Parameter, ParameterIn,
            header::{Header, is_ignored_header_parameter},
        },
        reference::RefOr,
    },
    validate::{
        rules::extensions::check_extensions,
        violation::{SpecError, Violation},
    },
};

pub(in crate::validate) fn check_parameter_list(
    location: &str,
    parameters: &[RefOr<Parameter>],
    violations: &mut Vec<Violation>,
) {
    let mut seen: HashSet<(String, ParameterIn)> = HashSet::new();

    for parameter in parameters.iter().filter_map(RefOr::as_item) {
        let key = (parameter.name.clone(), parameter.location);
        if !seen.insert(key) {
            violations.push(Violation::error(
                location,
                SpecError::DuplicateParameter {
                    name: parameter.name.clone(),
                    location: format!("{:?}", parameter.location).to_lowercase(),
                },
            ));
        }

        if parameter.location == ParameterIn::Header && is_ignored_header_parameter(&parameter.name)
        {
            violations.push(Violation::error(
                location,
                SpecError::IgnoredHeaderParameter {
                    name: parameter.name.clone(),
                },
            ));
        }

        if parameter.location == ParameterIn::Path && parameter.required != Some(true) {
            violations.push(Violation::error(
                location,
                SpecError::PathParameterNotRequired {
                    name: parameter.name.clone(),
                },
            ));
        }

        let has_schema = parameter.schema.is_some();
        let has_content = !parameter.content.is_empty();
        if has_schema == has_content {
            violations.push(Violation::error(
                location,
                SpecError::SchemaContentExclusivity {
                    name: parameter.name.clone(),
                },
            ));
        } else if has_content && parameter.content.len() != 1 {
            violations.push(Violation::error(
                location,
                SpecError::ContentNotSingular {
                    name: parameter.name.clone(),
                    found: parameter.content.len(),
                },
            ));
        }

        if let Some(style) = parameter.style {
            if !style.is_valid_for(parameter.location) {
                violations.push(Violation::error(
                    location,
                    SpecError::IllegalStyle {
                        style: format!("{style:?}").to_lowercase(),
                        location: format!("{:?}", parameter.location).to_lowercase(),
                    },
                ));
            }
        }

        if parameter.example.is_some() && !parameter.examples.is_empty() {
            violations.push(Violation::error(location, SpecError::ExampleExclusivity));
        }

        check_extensions(location, &parameter.extensions, violations);
    }
}

pub(in crate::validate) fn check_header(
    location: &str,
    name: &str,
    header: &Header,
    violations: &mut Vec<Violation>,
) {
    let has_schema = header.schema.is_some();
    let has_content = !header.content.is_empty();
    if has_schema == has_content {
        violations.push(Violation::error(
            location,
            SpecError::SchemaContentExclusivity {
                name: name.to_owned(),
            },
        ));
    } else if has_content && header.content.len() != 1 {
        violations.push(Violation::error(
            location,
            SpecError::ContentNotSingular {
                name: name.to_owned(),
                found: header.content.len(),
            },
        ));
    }

    if header.example.is_some() && !header.examples.is_empty() {
        violations.push(Violation::error(location, SpecError::ExampleExclusivity));
    }
}
