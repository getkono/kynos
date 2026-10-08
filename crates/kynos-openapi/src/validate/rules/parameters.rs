//! Parameter and header rules: uniqueness, the closed style table, and the
//! headers the specification refuses to describe.
//!
//! What is left here is what a name or a whole list settles and a single value
//! cannot: the schema/content exclusion and the style a header may declare are
//! both decided by construction now, and neither has a rule.

use std::collections::HashSet;

use crate::{
    Map,
    model::{
        components::Components,
        parameter::{
            Parameter, ParameterIn,
            header::{Header, is_ignored_header, is_ignored_header_parameter},
        },
        reference::RefOr,
    },
    validate::{
        rules::extensions::check_extensions,
        violation::{SpecError, Violation, pointer_token},
    },
};

/// The key a parameter is deduplicated under.
///
/// Header names are folded, everything else is compared as written.
fn fold_header_case(parameter: &Parameter) -> String {
    if parameter.location == ParameterIn::Header {
        parameter.name.to_ascii_lowercase()
    } else {
        parameter.name.clone()
    }
}

/// What one entry of a parameter list stands for, read against this document.
pub(in crate::validate) enum Resolved<'doc> {
    /// The entry is, or resolves to, this Parameter Object.
    Found(&'doc Parameter),
    /// A local reference to a component this document does not declare, or a
    /// cycle of them: there is definitely no parameter behind it.
    Missing,
    /// A reference this document alone cannot follow: another document, or a
    /// pointer that does not name a component. It may stand for any parameter.
    Elsewhere,
}

/// Follows `parameter` through `#/components/parameters` to the object it
/// stands for.
///
/// Only a whole component name is resolved. A name escaped with `~` or `%` is
/// read as [`Resolved::Elsewhere`] rather than decoded, since no valid
/// component name needs either (`check_component_names` reports one that
/// does).
pub(in crate::validate) fn resolve_parameter<'doc>(
    parameter: &'doc RefOr<Parameter>,
    components: &'doc Components,
) -> Resolved<'doc> {
    let mut current = parameter;
    // Each hop lands on a distinct component unless the chain cycles, so one
    // hop more than there are components proves a cycle.
    for _ in 0..=components.parameters.len() {
        let reference = match current {
            RefOr::Item(parameter) => return Resolved::Found(parameter),
            RefOr::Ref(reference) => reference,
        };
        let Some(name) = reference
            .location
            .strip_prefix("#/components/parameters/")
            .filter(|name| !name.contains(['/', '~', '%']))
        else {
            return Resolved::Elsewhere;
        };
        let Some(next) = components.parameters.get(name) else {
            return Resolved::Missing;
        };
        current = next;
    }
    Resolved::Missing
}

/// Checks one list of parameters, as written on a Path Item or an Operation.
///
/// Uniqueness is decided by what each entry resolves to, so a duplicate that
/// arrives by `$ref` is one. Every other rule here reads only the entries
/// written inline: a referenced component belongs to `components`, not to
/// this list, and checking it here would report it once per reference.
pub(in crate::validate) fn check_parameter_list(
    location: &str,
    parameters: &[RefOr<Parameter>],
    components: &Components,
    violations: &mut Vec<Violation>,
) {
    let mut seen: HashSet<(String, ParameterIn)> = HashSet::new();

    for entry in parameters {
        // An entry that resolves to nothing is skipped: the list holds no
        // parameter there to compare.
        let Resolved::Found(resolved) = resolve_parameter(entry, components) else {
            continue;
        };

        // A *field* name is case-insensitive (RFC 9110 section 5.1), which is
        // the same reading `is_ignored_header_parameter` already takes. A path,
        // query or cookie name is not, so only a header folds.
        let key = (fold_header_case(resolved), resolved.location);
        if !seen.insert(key) {
            violations.push(Violation::error(
                location,
                SpecError::DuplicateParameter {
                    name: resolved.name.clone(),
                    location: format!("{:?}", resolved.location).to_lowercase(),
                },
            ));
        }

        let Some(parameter) = entry.as_item() else {
            continue;
        };

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

/// Checks the headers of a response or of an encoded part.
///
/// The name is the only thing here a `Header` cannot settle on its own: its
/// shape, its examples and now its style are all decided by construction, but
/// the name is a key in the surrounding map and no value's type reaches it.
///
/// [`Components::headers`](crate::Components::headers) is deliberately not
/// checked. A reusable header is not yet in a response or an encoding, so
/// nothing has stated its media type separately and there is nothing for it to
/// contradict.
pub(in crate::validate) fn check_header_map(
    location: &str,
    headers: &Map<RefOr<Header>>,
    violations: &mut Vec<Violation>,
) {
    for (name, header) in headers {
        if is_ignored_header(name) {
            violations.push(Violation::error(
                location,
                SpecError::IgnoredHeader { name: name.clone() },
            ));
        }

        if let Some(header) = header.as_item() {
            check_extensions(
                &format!("{location}/{}", pointer_token(name)),
                &header.extensions,
                violations,
            );
        }
    }
}
