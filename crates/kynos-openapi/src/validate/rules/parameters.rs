//! Parameter and header rules: uniqueness, the closed style table, the headers
//! the specification refuses to describe, and what `in: querystring` excludes.
//!
//! What is left here is what a name or a whole list settles and a single value
//! cannot: the schema/content exclusion and the style a header may declare are
//! both decided by construction now, and neither has a rule.

use std::collections::HashSet;

use crate::{
    Map,
    model::{
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

pub(in crate::validate) fn check_parameter_list(
    location: &str,
    parameters: &[RefOr<Parameter>],
    violations: &mut Vec<Violation>,
) {
    let mut seen: HashSet<(String, ParameterIn)> = HashSet::new();

    for parameter in parameters.iter().filter_map(RefOr::as_item) {
        // A *field* name is case-insensitive (RFC 9110 section 5.1), which is
        // the same reading `is_ignored_header_parameter` already takes. A path,
        // query or cookie name is not, so only a header folds.
        let key = (fold_header_case(parameter), parameter.location);
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

/// Checks the three rules 3.2 states for `in: querystring`: it is described
/// by `content`, at most one applies to an operation, and none applies beside
/// an `in: query` parameter.
///
/// `own` is the list at `location`; `inherited` is the path item's list when
/// `own` is an operation's, and empty when `own` is the path item's. The last
/// two rules hold "in the same operation (or in the operation's path-item)",
/// so they read the parameters that apply: the inherited ones the operation
/// does not override, then its own. A pair drawn wholly from `inherited` is
/// skipped, because the path item's own check already reported it once, at
/// the path item.
#[cfg(feature = "openapi32")]
pub(in crate::validate) fn check_querystring(
    location: &str,
    inherited: &[RefOr<Parameter>],
    own: &[RefOr<Parameter>],
    violations: &mut Vec<Violation>,
) {
    let own: Vec<&Parameter> = own.iter().filter_map(RefOr::as_item).collect();

    for parameter in &own {
        if parameter.location == ParameterIn::Querystring && parameter.content().is_none() {
            violations.push(Violation::error(
                location,
                SpecError::QuerystringWithoutContent {
                    name: parameter.name.clone(),
                },
            ));
        }
    }

    let overridden = |parameter: &Parameter| {
        own.iter().any(|mine| {
            mine.location == parameter.location
                && fold_header_case(mine) == fold_header_case(parameter)
        })
    };
    // Each applying parameter, and whether it is one this location declares.
    let applying: Vec<(&Parameter, bool)> = inherited
        .iter()
        .filter_map(RefOr::as_item)
        .filter(|parameter| !overridden(parameter))
        .map(|parameter| (parameter, false))
        .chain(own.iter().map(|parameter| (*parameter, true)))
        .collect();
    let of = |location: ParameterIn| {
        applying
            .iter()
            .filter(move |(parameter, _)| parameter.location == location)
    };

    let mut querystrings = of(ParameterIn::Querystring);
    let Some(&(first, first_is_own)) = querystrings.next() else {
        return;
    };

    for &(second, second_is_own) in querystrings {
        if first_is_own || second_is_own {
            violations.push(Violation::error(
                location,
                SpecError::DuplicateQuerystring {
                    first: first.name.clone(),
                    second: second.name.clone(),
                },
            ));
        }
    }

    // Each query parameter once, naming the first querystring parameter it is
    // newly beside here.
    for &(query, query_is_own) in of(ParameterIn::Query) {
        if let Some(&(querystring, _)) = of(ParameterIn::Querystring)
            .find(|&&(_, querystring_is_own)| query_is_own || querystring_is_own)
        {
            violations.push(Violation::error(
                location,
                SpecError::QueryBesideQuerystring {
                    query: query.name.clone(),
                    querystring: querystring.name.clone(),
                },
            ));
        }
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
