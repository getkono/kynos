//! What [`Named`] and [`Lone`] do: read their value out of what a match
//! captured, describe it, and write it into a typed URI.

use std::borrow::Cow;

use super::{
    DecodePath, EncodeBinding, EncodePath, Lone, Named, PathBinding,
    captures::{INLINE, PathCaptures},
};
use crate::{
    error::rejection::PathRejection,
    http::Parts,
    schema::{
        ParamValue,
        constraints::{ConstraintProblem, Pointer, Violations},
        registry::Registry,
    },
};

impl<T: DecodePath> PathBinding<T> for Named<T> {
    const NAMES: Option<&'static [&'static str]> = Some(T::NAMES);

    fn parameters(registry: &mut Registry, _: &[String]) -> Vec<kynos_openapi::Parameter> {
        T::parameters(registry)
    }

    fn constraint_problems() -> Vec<ConstraintProblem> {
        T::constraint_problems()
    }

    fn decode(parts: &Parts) -> Result<T, PathRejection> {
        let path = parts.uri.path();
        let captures = captures(parts);

        let names = T::NAMES;
        if names.len() <= INLINE {
            let mut decoded: [Cow<'_, str>; INLINE] = Default::default();
            let mut values = [("", ""); INLINE];
            let values = decode_captures(names, captures, path, &mut decoded, &mut values)?;
            T::decode(values)
        } else {
            let mut decoded = vec![Cow::Borrowed(""); names.len()];
            let mut values = vec![("", ""); names.len()];
            let values = decode_captures(names, captures, path, &mut decoded, &mut values)?;
            T::decode(values)
        }
    }
}

impl<T: EncodePath> EncodeBinding<T> for Named<T> {
    fn encode(value: &T, _: &[&'static str]) -> Vec<(&'static str, String)> {
        value.encode()
    }
}

impl<T: ParamValue> PathBinding<T> for Lone<T> {
    const NAMES: Option<&'static [&'static str]> = None;

    fn parameters(registry: &mut Registry, variables: &[String]) -> Vec<kynos_openapi::Parameter> {
        if let [name] = variables {
            return vec![kynos_openapi::Parameter::path(
                name.as_str(),
                registry.resolve::<T>(),
            )];
        }
        registry.refuse_parameter(crate::router::error::RouterError::LonePathValue {
            value: std::any::type_name::<T>(),
            variables: variables.to_vec(),
        });
        Vec::new()
    }

    fn constraint_problems() -> Vec<ConstraintProblem> {
        T::CONSTRAINT_PROBLEM.into_iter().collect()
    }

    fn decode(parts: &Parts) -> Result<T, PathRejection> {
        let path = parts.uri.path();
        let captures = captures(parts);
        // The operation's own variables follow every prefix's, so its one
        // variable is the last the match captured.
        let name = captures.and_then(PathCaptures::last).unwrap_or_default();
        let text = decode_capture(name, captures, path)?;

        let value = T::from_str(&text).map_err(|error| PathRejection::Invalid {
            name: name.to_owned(),
            detail: error.to_string(),
        })?;

        let mut violations = Violations::new();
        value.check_constraints(Pointer::root(), &mut violations);
        if !violations.is_empty() {
            return Err(PathRejection::Schema {
                name: name.to_owned(),
                failures: violations.into_failures(),
                problem: T::CONSTRAINT_PROBLEM,
            });
        }
        Ok(value)
    }
}

impl<T: ParamValue> EncodeBinding<T> for Lone<T> {
    fn encode(value: &T, variables: &[&'static str]) -> Vec<(&'static str, String)> {
        let [name] = variables else {
            panic!(
                "one path value fills a template's one variable, and this template declares \
                 {variables:?}"
            );
        };
        vec![(*name, value.to_string())]
    }
}

/// What the matched route captured, where it captured anything.
fn captures(parts: &Parts) -> Option<&PathCaptures> {
    parts
        .extensions
        .get::<crate::router::dispatch::Routed>()
        .and_then(|routed| routed.captures.as_ref())
}

/// The value `captures` holds for the variable `name`, percent-decoded.
fn decode_capture<'p>(
    name: &str,
    captures: Option<&PathCaptures>,
    path: &'p str,
) -> Result<Cow<'p, str>, PathRejection> {
    let raw = captures
        .and_then(|captures| captures.get(path, name))
        .ok_or_else(|| PathRejection::Invalid {
            name: name.to_owned(),
            detail: "the matched route captured no value for this variable".to_owned(),
        })?;
    crate::__private::uri::decode_path_value(raw).map_err(|error| PathRejection::Invalid {
        name: name.to_owned(),
        detail: format!("the percent-decoded value is not valid UTF-8: {error}"),
    })
}

/// Decodes the value `captures` holds for each of `names` into `decoded`,
/// then pairs each name with its value in `values`, which it returns.
///
/// The caller provides both, so a group of up to [`INLINE`] variables keeps
/// them on its stack.
fn decode_captures<'v, 'p: 'v>(
    names: &'static [&'static str],
    captures: Option<&PathCaptures>,
    path: &'p str,
    decoded: &'v mut [Cow<'p, str>],
    values: &'v mut [(&'v str, &'v str)],
) -> Result<&'v [(&'v str, &'v str)], PathRejection> {
    for (name, slot) in names.iter().zip(decoded.iter_mut()) {
        *slot = decode_capture(name, captures, path)?;
    }

    let decoded = &*decoded;
    for (pair, (name, value)) in values.iter_mut().zip(names.iter().zip(decoded)) {
        *pair = (name, value.as_ref());
    }
    Ok(&values[..names.len()])
}
