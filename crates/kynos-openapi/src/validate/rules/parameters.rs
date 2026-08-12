//! Parameter and header rules: uniqueness, the schema/content exclusion, the
//! closed style table, and the headers the specification refuses to describe.

use std::collections::HashSet;

use crate::{
    model::{
        parameter::{Parameter, ParameterIn, header::is_ignored_header_parameter},
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

        // The schema/content exclusion and the single-entry `content` rule used
        // to be checked here. `ParameterShape` holds one or the other and its
        // `Content` variant holds one pair, so neither violation can reach this
        // function.

        if let Some(style) = parameter.style() {
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

        // The `example`/`examples` exclusion used to be checked here too. A
        // parameter carries one `Examples` holding one form or the other, so
        // that violation cannot reach this function either.

        check_extensions(location, &parameter.extensions, violations);
    }
}
