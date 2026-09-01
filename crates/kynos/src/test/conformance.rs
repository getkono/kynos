//! Checking one observed response against what the description promised.
//!
//! Private, and named by no path a user can write: everything here is reached
//! through [`TestClient::assert_conformance`](super::TestClient::assert_conformance)
//! and [`TestClient::assert_declared_responses_covered`](super::TestClient::assert_declared_responses_covered),
//! which are the two assertions this module exists to answer.
//!
//! `jsonschema` is named here and nowhere else, which is the allowance
//! `docs/architecture.md` gives it.

use kynos_openapi::{
    Document, Method, RefOr, Responses, StatusPattern, model::parameter::header::is_ignored_header,
};
use serde_json::Value;

use crate::{
    http::{HeaderMap, header},
    test::Observed,
};

/// Every way one observed response fails to match the description.
///
/// A list rather than the first failure, so that one run reports everything a
/// response got wrong instead of one thing per run.
pub(super) fn conformance(document: &Document, record: &Observed) -> Vec<String> {
    let Some(template) = matched_template(document, &record.path) else {
        return vec!["no declared path matches this request".to_owned()];
    };
    let Some(method) = Method::from_wire_str(record.method.as_str()) else {
        return vec![format!(
            "`{}` is not a method a description can declare",
            record.method
        )];
    };
    let Some(operation) = document
        .paths
        .items
        .get(template)
        .and_then(|item| item.operation(method))
    else {
        return vec![format!(
            "`{template}` declares no `{}` operation",
            record.method
        )];
    };

    let status = record.status.as_u16();
    let Some((key, entry)) = declared_response(&operation.responses, status) else {
        return vec![format!("status {status} is not declared")];
    };
    let Some(response) = resolve_response(document, entry) else {
        return vec![format!("the `{key}` response resolves to no component")];
    };

    let mut reasons = Vec::new();

    for (name, declared) in &response.headers {
        // A `Content-Type` entry is ignored by the specification: `content`
        // already states it.
        if is_ignored_header(name) {
            continue;
        }
        let required = match declared {
            RefOr::Item(header) => header.required,
            RefOr::Ref(reference) => reference
                .location
                .strip_prefix("#/components/headers/")
                .and_then(|name| document.components.headers.get(name))
                .and_then(RefOr::as_item)
                .and_then(|header| header.required),
        };
        if required == Some(true) && !record.headers.contains_key(name.as_str()) {
            reasons.push(format!(
                "the declared required header `{name}` was not sent"
            ));
        }
    }

    reasons.extend(body_conformance(document, response, record));
    reasons
}

/// Whether the body matches the representation declared for it.
///
/// # Declaring nothing is a claim too
///
/// A response with no `content` says the exchange carried no representation,
/// which is checked against what was sent rather than taken as nothing to
/// check. Keyed on the exchange in both directions: octets are a
/// representation whatever the headers said, and a `Content-Type` names one
/// whether or not any octets followed.
///
/// Every shape Kynos ships that legitimately declares nothing sends neither, so
/// none of them reaches the report below: `NoContent`'s 204 and every
/// `Redirect<CODE>` build a `Body::empty()`; the conditional 304 copies a
/// replayed field list that deliberately omits `Content-Type`; the ranged 304
/// guards its media type behind the status; the asset 304 writes only `ETag`,
/// `Cache-Control`, `Content-Encoding` and `Vary`; the CORS preflight 204
/// writes only `Vary` and `Access-Control-*`; and a HEAD keeps its
/// `Content-Type` on statuses that *do* declare a representation, so it never
/// takes this branch.
fn body_conformance(
    document: &Document,
    response: &kynos_openapi::Response,
    record: &Observed,
) -> Vec<String> {
    let observed = media_type(&record.headers);

    if response.content.is_empty() {
        if record.body.is_empty() && observed.is_none() {
            return Vec::new();
        }

        return vec![format!(
            "the description declares no content, but {} was sent",
            match (&observed, record.body.len()) {
                (Some(media_type), 0) => format!("a `{media_type}` head with no body"),
                (Some(media_type), len) => format!("a {len}-byte `{media_type}` body"),
                (None, len) => format!("a {len}-byte body with no `Content-Type`"),
            }
        )];
    }

    let declared = || {
        response
            .content
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    };

    let Some(media_type) = observed else {
        return vec![format!(
            "no `Content-Type` was sent, but the description declares {}",
            declared()
        )];
    };
    let Some(representation) = declared_representation(&response.content, &media_type) else {
        return vec![format!(
            "`{media_type}` is not a declared representation; the description declares {}",
            declared()
        )];
    };

    let Some(schema) = representation.schema.as_ref() else {
        return Vec::new();
    };
    // Only a JSON-based representation has an instance a JSON Schema can be
    // applied to. Anything else is declared with a schema describing a shape
    // this module has no decoder for, and asserting nothing beats asserting
    // something wrong.
    if media_type != "application/json" && !media_type.ends_with("+json") {
        return Vec::new();
    }

    let instance = match serde_json::from_slice::<Value>(&record.body) {
        Ok(instance) => instance,
        Err(error) => return vec![format!("the body is not valid JSON: {error}")],
    };

    validate(document, schema, &instance)
}

/// Validates one instance against one declared schema.
///
/// The schema is lifted into a document of its own carrying the description's
/// `components`, so that a `$ref` such as `#/components/schemas/User` resolves
/// against the description rather than against nothing — without which every
/// referenced schema would silently accept every body.
fn validate(document: &Document, schema: &kynos_openapi::Schema, instance: &Value) -> Vec<String> {
    let mut root = serde_json::to_value(schema).expect("a schema in a document is serializable");

    if let Value::Object(members) = &mut root {
        // The OAS dialect is not a meta-schema this validator knows, and
        // retrieving one is off: OpenAPI 3.1 and 3.2 schemas are JSON Schema
        // 2020-12, which is what the validator is built for below.
        members.remove("$schema");
        members.insert(
            "components".to_owned(),
            serde_json::to_value(&document.components)
                .expect("a components object in a document is serializable"),
        );
    }

    let validator = match jsonschema::draft202012::new(&root) {
        Ok(validator) => validator,
        Err(error) => return vec![format!("the declared schema does not compile: {error}")],
    };

    validator
        .iter_errors(instance)
        .map(|error| format!("the body does not match the declared schema: {error}"))
        .collect()
}

/// The `paths` key this request matched, or `None` when the description
/// declares no path it could have reached.
///
/// The most literal template wins, and document order breaks a tie — the same
/// order of preference the matcher applies, restated here because a response
/// carries no record of the route that produced it.
pub(super) fn matched_template<'d>(document: &'d Document, path: &str) -> Option<&'d str> {
    let path = path.split(['?', '#']).next().unwrap_or(path);

    let mut best: Option<(&'d String, usize)> = None;
    for template in document.paths.items.keys() {
        if !template_matches(template, path) {
            continue;
        }
        let literals = template
            .split('/')
            .filter(|segment| !is_variable(segment))
            .count();
        if best.is_none_or(|(_, most)| literals > most) {
            best = Some((template, literals));
        }
    }

    best.map(|(template, _)| template.as_str())
}

/// Whether a concrete path is an instance of a template.
///
/// A template expression always spans a whole segment — the path grammar
/// [`PathTemplate`](kynos_openapi::PathTemplate) accepts allows nothing else —
/// so this compares segment by segment.
fn template_matches(template: &str, path: &str) -> bool {
    let mut expected = template.split('/');
    let mut actual = path.split('/');

    loop {
        match (expected.next(), actual.next()) {
            (None, None) => return true,
            (Some(expected), Some(actual)) => {
                if is_variable(expected) {
                    // A variable stands for a segment, and a segment holds at
                    // least one character.
                    if actual.is_empty() {
                        return false;
                    }
                } else if expected != actual {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// Whether a template segment is a `{}` expression.
fn is_variable(segment: &str) -> bool {
    segment.starts_with('{') && segment.ends_with('}')
}

/// The entry declared for a status, and the key that declared it.
///
/// An exact code first, then a wildcard, then `default` — the precedence the
/// specification gives a consumer resolving a status against `responses`.
pub(super) fn declared_response(
    responses: &Responses,
    status: u16,
) -> Option<(String, &RefOr<kynos_openapi::Response>)> {
    let exact = StatusPattern::Code(status).to_string();
    if let Some(entry) = responses.responses.get(&exact) {
        return Some((exact, entry));
    }

    for (key, entry) in &responses.responses {
        if key
            .parse::<StatusPattern>()
            .is_ok_and(|pattern| pattern.matches(status))
        {
            return Some((key.clone(), entry));
        }
    }

    responses
        .default_response
        .as_ref()
        .map(|entry| ("default".to_owned(), entry))
}

/// Every key a set of responses declares, `default` included.
pub(super) fn declared_keys(responses: &Responses) -> Vec<String> {
    let mut keys: Vec<String> = responses.responses.keys().cloned().collect();
    if responses.default_response.is_some() {
        keys.push("default".to_owned());
    }
    keys
}

/// The response an entry names, following one `#/components/responses` hop.
fn resolve_response<'d>(
    document: &'d Document,
    entry: &'d RefOr<kynos_openapi::Response>,
) -> Option<&'d kynos_openapi::Response> {
    match entry {
        RefOr::Item(response) => Some(response),
        RefOr::Ref(reference) => reference
            .location
            .strip_prefix("#/components/responses/")
            .and_then(|name| document.components.responses.get(name))
            .and_then(RefOr::as_item),
    }
}

/// The media type a response stated, lowercased and without its parameters.
/// The representation declared for `media_type`, if one is.
///
/// An exact match first, then one ignoring the declared key's parameters. Both
/// halves are needed. A description may declare two representations differing
/// only by parameter -- `text/plain; charset=utf-8` beside a legacy charset --
/// and stripping first would hand back whichever came first in the map. But a
/// `Content-Type` is compared with its own parameters already stripped, so
/// without the fallback a declared `text/html; charset=utf-8` could never
/// match anything, and every parameterized media type in the document would
/// read as undeclared.
fn declared_representation<'a>(
    content: &'a kynos_openapi::Map<kynos_openapi::MediaType>,
    media_type: &str,
) -> Option<&'a kynos_openapi::MediaType> {
    content
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(media_type))
        .or_else(|| {
            content.iter().find(|(name, _)| {
                let (base, _) = name.split_once(';').unwrap_or((name.as_str(), ""));
                base.trim().eq_ignore_ascii_case(media_type)
            })
        })
        .map(|(_, representation)| representation)
}

fn media_type(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::CONTENT_TYPE)?.to_str().ok()?;
    let (media_type, _) = value.split_once(';').unwrap_or((value, ""));
    Some(media_type.trim().to_ascii_lowercase())
}

#[cfg(test)]
mod tests;
