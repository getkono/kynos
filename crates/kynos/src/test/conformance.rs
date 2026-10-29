//! Checking one observed response against what the description promised.
//!
//! `jsonschema` is named here and nowhere else, per `docs/architecture.md`.

use kynos_openapi::{
    Document, Method, RefOr, Responses, StatusPattern,
    model::{body::mime_names, parameter::header::is_ignored_header},
};
use serde_json::Value;

use crate::{
    http::{HeaderMap, header},
    test::Observed,
};

/// Every way one observed response fails to match the description.
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
    // A HEAD on a path declaring no `head` was answered by the `get`, which is
    // the operation describing it (RFC 9110 section 9.3.2).
    let Some(operation) = document.paths.items.get(template).and_then(|item| {
        item.operation(method).or_else(|| {
            if method == Method::Head {
                item.operation(Method::Get)
            } else {
                None
            }
        })
    }) else {
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
        // The specification ignores a `Content-Type` entry.
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
/// A response declaring no `content` is a claim too: any octets or any
/// `Content-Type` under it is reported. A HEAD carries no content (RFC 9110
/// section 9.3.2), so only its `Content-Type` is held to the declaration.
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
    let Some((key, entry)) = declared_representation(&response.content, &media_type) else {
        return vec![format!(
            "`{media_type}` is not a declared representation; the description declares {}",
            declared()
        )];
    };
    let Some(representation) = resolve_media_type(document, entry) else {
        return vec![format!(
            "the `{key}` representation resolves to no component"
        )];
    };

    // RFC 9110 section 9.3.2: a HEAD's content is never sent.
    if record.method == crate::http::Method::HEAD {
        if record.body.is_empty() {
            return Vec::new();
        }
        return vec![format!(
            "a {}-byte body was sent in response to a HEAD",
            record.body.len()
        )];
    }

    let Some(schema) = representation.schema.as_ref() else {
        return Vec::new();
    };
    // Only a JSON-based representation can be validated against its schema.
    if media_type != mime_names::APPLICATION_JSON && !media_type.ends_with("+json") {
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
/// The schema carries the description's `components`, so a `$ref` resolves
/// rather than silently accepting every body.
fn validate(document: &Document, schema: &kynos_openapi::Schema, instance: &Value) -> Vec<String> {
    let mut root = serde_json::to_value(schema).expect("a schema in a document is serializable");

    if let Value::Object(members) = &mut root {
        // The validator does not know the OAS dialect; it is 2020-12 anyway.
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
/// Between matching templates, the first position where their segments rank
/// differently decides, read left to right, as in the router's matcher: a
/// literal beats a segment mixing an expression with literal text, which beats
/// a whole-segment variable, and between two mixed segments the longer literal
/// text wins. Document order breaks a tie, which only templates the router
/// would refuse as conflicting can reach.
pub(super) fn matched_template<'d>(document: &'d Document, path: &str) -> Option<&'d str> {
    let path = path.split(['?', '#']).next().unwrap_or(path);

    let mut best: Option<&'d str> = None;
    for template in document.paths.items.keys() {
        if template_matches(template, path) && best.is_none_or(|best| outranks(template, best)) {
            best = Some(template);
        }
    }

    best
}

/// Whether `candidate` takes priority over `incumbent`, two templates matching
/// the same path: the first segment where the two rank differently goes to the
/// higher.
fn outranks(candidate: &str, incumbent: &str) -> bool {
    candidate
        .split('/')
        .zip(incumbent.split('/'))
        .map(|(candidate, incumbent)| rank(candidate).cmp(&rank(incumbent)))
        .find(|ordering| ordering.is_ne())
        .is_some_and(std::cmp::Ordering::is_gt)
}

/// How the router ranks a template segment against another in its position.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    /// A segment holding an expression, by the length of its literal text:
    /// zero for a whole-segment variable, more for a mixed segment.
    Variable(usize),
    /// A segment holding no expression.
    Literal,
}

/// The rank of one template segment.
fn rank(segment: &str) -> Rank {
    if segment.contains('{') {
        // A literal run, then an expression name, alternately.
        Rank::Variable(segment.split(['{', '}']).step_by(2).map(str::len).sum())
    } else {
        Rank::Literal
    }
}

/// Whether a concrete path is an instance of a template.
///
/// Segment by segment, since a [`PathTemplate`](kynos_openapi::PathTemplate)
/// expression never spans a `/`.
fn template_matches(template: &str, path: &str) -> bool {
    let mut expected = template.split('/');
    let mut actual = path.split('/');

    loop {
        match (expected.next(), actual.next()) {
            (None, None) => return true,
            (Some(expected), Some(actual)) => {
                if !segment_matches(expected, actual) {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// Whether a concrete path segment is an instance of a template segment.
///
/// The literal text is matched exactly and each expression stands for a
/// non-empty run of characters. The router serves at most one expression per
/// segment; a crowded one gives each literal between two expressions the first
/// place it fits, which leaves the most room for those after it.
fn segment_matches(expected: &str, actual: &str) -> bool {
    // A literal run, then an expression name, alternately.
    let mut literals = expected.split(['{', '}']).step_by(2);
    let first = literals.next().unwrap_or_default();
    let Some(mut rest) = actual.strip_prefix(first) else {
        return false;
    };

    let mut literals = literals.peekable();
    while let Some(literal) = literals.next() {
        // The expression before `literal` takes at least one character.
        let mut characters = rest.chars();
        if characters.next().is_none() {
            return false;
        }
        let after = characters.as_str();

        if literals.peek().is_none() {
            return after.ends_with(literal);
        }
        let Some(at) = after.find(literal) else {
            return false;
        };
        rest = &after[at + literal.len()..];
    }

    rest.is_empty()
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

/// The representation declared for `media_type`, if one is.
///
/// A key may be a media range, and only the most specific matching key
/// applies: the exact key, so two keys differing only by parameter stay
/// distinct, then one ignoring the key's parameters, since `media_type`
/// arrives with its own already stripped, then `type/*`, then `*/*`. Keys
/// equally specific resolve to the first the map holds.
fn declared_representation<'a>(
    content: &'a kynos_openapi::Map<RefOr<kynos_openapi::MediaType>>,
    media_type: &str,
) -> Option<(&'a str, &'a RefOr<kynos_openapi::MediaType>)> {
    content
        .iter()
        .enumerate()
        .filter_map(|(index, (name, entry))| {
            Some((
                specificity(name, media_type)?,
                index,
                (name.as_str(), entry),
            ))
        })
        .max_by_key(|&(rank, index, _)| (rank, std::cmp::Reverse(index)))
        .map(|(_, _, declared)| declared)
}

/// How specifically the `content` key `name` matches `media_type`, if at all.
fn specificity(name: &str, media_type: &str) -> Option<u8> {
    if name.eq_ignore_ascii_case(media_type) {
        return Some(3);
    }

    let (range, _) = name.split_once(';').unwrap_or((name, ""));
    let range = range.trim();
    if range.eq_ignore_ascii_case(media_type) {
        return Some(2);
    }

    // A range covers only a `type/subtype` value (RFC 9110 section 8.3.1).
    let (observed_type, observed_subtype) = media_type.split_once('/')?;
    if observed_type.is_empty() || observed_subtype.is_empty() {
        return None;
    }
    if range == "*/*" {
        return Some(0);
    }

    let (range_type, subtype) = range.split_once('/')?;
    (subtype == "*" && range_type.eq_ignore_ascii_case(observed_type)).then_some(1)
}

/// The media type an entry names, following one `#/components/mediaTypes`
/// hop; a build without `openapi32` has no such section to follow.
fn resolve_media_type<'d>(
    #[cfg_attr(not(feature = "openapi32"), expect(unused_variables))] document: &'d Document,
    entry: &'d RefOr<kynos_openapi::MediaType>,
) -> Option<&'d kynos_openapi::MediaType> {
    match entry {
        RefOr::Item(media_type) => Some(media_type),
        #[cfg(feature = "openapi32")]
        RefOr::Ref(reference) => reference
            .location
            .strip_prefix("#/components/mediaTypes/")
            .and_then(|name| document.components.media_types.get(name))
            .and_then(RefOr::as_item),
        #[cfg(not(feature = "openapi32"))]
        RefOr::Ref(_) => None,
    }
}

/// The media type a response stated, lowercased and without its parameters.
fn media_type(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::CONTENT_TYPE)?.to_str().ok()?;
    let (media_type, _) = value.split_once(';').unwrap_or((value, ""));
    Some(media_type.trim().to_ascii_lowercase())
}

#[cfg(test)]
mod tests;
