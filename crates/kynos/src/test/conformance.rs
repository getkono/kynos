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
        .0
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
fn body_conformance(
    document: &Document,
    response: &kynos_openapi::Response,
    record: &Observed,
) -> Vec<String> {
    if response.content.is_empty() {
        return Vec::new();
    }

    let declared = || {
        response
            .content
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    };

    let Some(media_type) = media_type(&record.headers) else {
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
    for template in document.paths.0.keys() {
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
mod tests {
    use kynos_openapi::{Document, Operation, PathItem, PathTemplate};

    use super::{declared_response, matched_template, template_matches};

    /// The segments a template may be built from. The variable is a marker
    /// rather than a name: a template may not repeat one, so each is numbered
    /// by position before the template is used.
    const TEMPLATE_ALPHABET: [&str; 3] = ["a", "b", "{}"];
    /// The segments a concrete path may be built from. `""` is here because a
    /// double slash is a path a client can send and a variable must not accept.
    const PATH_ALPHABET: [&str; 4] = ["a", "b", "x", ""];
    /// What a variable stands for, which is every non-empty path segment.
    const SUBSTITUTIONS: [&str; 3] = ["a", "b", "x"];

    /// Every sequence of `1..=3` segments over `alphabet`, as a rooted path.
    fn rooted(alphabet: &[&str]) -> Vec<String> {
        let mut all = Vec::new();
        for length in 1..=3 {
            let mut sequences: Vec<Vec<&str>> = vec![Vec::new()];
            for _ in 0..length {
                sequences = sequences
                    .into_iter()
                    .flat_map(|prefix| {
                        alphabet.iter().map(move |segment| {
                            let mut next = prefix.clone();
                            next.push(segment);
                            next
                        })
                    })
                    .collect();
            }
            all.extend(
                sequences
                    .into_iter()
                    .map(|parts| format!("/{}", parts.join("/"))),
            );
        }
        all
    }

    /// Numbers each `{}` marker by position, so no template repeats a variable
    /// name — which `PathTemplate` refuses, and rightly.
    fn named(template: &str) -> String {
        let mut next = 0;
        template
            .split('/')
            .map(|segment| {
                if segment == "{}" {
                    next += 1;
                    format!("{{v{}}}", next - 1)
                } else {
                    segment.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Every concrete path a template accepts, built by *substitution*.
    ///
    /// The independently constructed oracle the parser rule asks for: it
    /// expands where `template_matches` compares, so the two agree only where
    /// both are right. An oracle that walked the segments the same way would
    /// agree with the matcher by construction, including wherever both were
    /// wrong.
    fn instances(template: &str) -> Vec<String> {
        let mut expanded: Vec<Vec<&str>> = vec![Vec::new()];

        for segment in template.trim_start_matches('/').split('/') {
            expanded = if segment.starts_with('{') && segment.ends_with('}') {
                expanded
                    .into_iter()
                    .flat_map(|prefix| {
                        SUBSTITUTIONS.iter().map(move |value| {
                            let mut next = prefix.clone();
                            next.push(value);
                            next
                        })
                    })
                    .collect()
            } else {
                expanded
                    .into_iter()
                    .map(|mut prefix| {
                        prefix.push(segment);
                        prefix
                    })
                    .collect()
            };
        }

        expanded
            .into_iter()
            .map(|parts| format!("/{}", parts.join("/")))
            .collect()
    }

    /// A total sweep rather than a property.
    ///
    /// The input space is finite and small — 39 templates against 84 paths —
    /// and `docs/testing.md` says a sweep is the stronger statement there,
    /// because it is total where a draw from the same space is a sample of it.
    /// It is also what keeps `proptest` out of `crates/kynos`'s
    /// dev-dependencies, which matters: five UI snapshots embed rustc's "the
    /// following other types implement" list.
    #[test]
    fn a_template_matches_exactly_the_paths_substituting_into_it_produces() {
        let templates: Vec<String> = rooted(&TEMPLATE_ALPHABET)
            .iter()
            .map(|t| named(t))
            .collect();
        let paths = rooted(&PATH_ALPHABET);

        assert_eq!(templates.len(), 39);
        assert_eq!(paths.len(), 84);

        for template in &templates {
            let accepted = instances(template);

            for path in &paths {
                let expected = accepted.contains(path);
                assert_eq!(
                    template_matches(template, path),
                    expected,
                    "`{template}` against `{path}`"
                );
            }
        }
    }

    /// Every template in the sweep is one the model would accept, so the sweep
    /// is over the language `PathTemplate` defines rather than over strings
    /// that only look like it.
    #[test]
    fn every_template_in_the_sweep_is_one_the_model_parses() {
        for template in rooted(&TEMPLATE_ALPHABET).iter().map(|t| named(t)) {
            assert!(
                PathTemplate::parse(&template).is_ok(),
                "`{template}` is not a path template"
            );
        }
    }

    fn document(templates: &[&str]) -> Document {
        let mut document = Document::new(
            kynos_openapi::SpecVersion::V3_1,
            kynos_openapi::Info::new("Fixture", "1.0.0"),
        );
        for template in templates {
            document.paths.0.insert(
                (*template).to_owned(),
                PathItem {
                    get: Some(Box::new(Operation::default())),
                    ..PathItem::default()
                },
            );
        }
        document
    }

    /// The more literal of two matching templates wins, which is what keeps a
    /// declared `/users/me` from being answered by `/users/{id}`'s description.
    #[test]
    fn the_most_literal_matching_template_is_the_one_chosen() {
        let document = document(&["/users/{id}", "/users/me"]);

        assert_eq!(matched_template(&document, "/users/me"), Some("/users/me"));
        assert_eq!(
            matched_template(&document, "/users/42"),
            Some("/users/{id}")
        );
    }

    /// A request target carries a query and may carry a fragment; a `paths` key
    /// carries neither, so both are cut before matching.
    #[test]
    fn a_query_or_fragment_is_not_part_of_what_is_matched() {
        let document = document(&["/users"]);

        assert_eq!(
            matched_template(&document, "/users?limit=2"),
            Some("/users")
        );
        assert_eq!(matched_template(&document, "/users#top"), Some("/users"));
        assert_eq!(matched_template(&document, "/other"), None);
    }

    /// The precedence a consumer resolves a status by: exact, then wildcard,
    /// then `default`. All three, and the miss.
    #[test]
    fn a_status_resolves_to_the_most_specific_key_declaring_it() {
        use kynos_openapi::{RefOr, Response, Responses};

        let responses = Responses::new()
            .with(200, Response::new("exact"))
            .with_pattern(
                kynos_openapi::StatusPattern::Success,
                RefOr::Item(Response::new("wildcard")),
            );

        assert_eq!(
            declared_response(&responses, 200).map(|(key, _)| key),
            Some("200".to_owned())
        );
        assert_eq!(
            declared_response(&responses, 204).map(|(key, _)| key),
            Some("2XX".to_owned())
        );
        assert_eq!(declared_response(&responses, 404).map(|(key, _)| key), None);

        let mut with_default = responses;
        with_default.default_response = Some(RefOr::Item(Response::new("default")));
        assert_eq!(
            declared_response(&with_default, 404).map(|(key, _)| key),
            Some("default".to_owned())
        );
    }

    /// One `content` map, from the media types it declares.
    fn content(declared: &[&str]) -> kynos_openapi::Map<kynos_openapi::MediaType> {
        declared
            .iter()
            .map(|name| {
                (
                    (*name).to_owned(),
                    kynos_openapi::MediaType::new(kynos_openapi::Schema::Object(Box::default())),
                )
            })
            .collect()
    }

    /// A `Content-Type` is compared with its parameters already stripped, so a
    /// declared media type carrying one has to match the bare form -- or every
    /// parameterized entry in a document reads as undeclared. `text/html` and
    /// `text/css` both reach the document that way through `router::assets`.
    #[test]
    fn a_declared_media_type_matches_despite_its_parameters() {
        let declared = content(&["text/html; charset=utf-8"]);

        assert!(super::declared_representation(&declared, "text/html").is_some());
    }

    /// The exact match is tried first, so two representations differing only by
    /// parameter resolve to the one actually named rather than to whichever the
    /// map happened to hold first.
    #[test]
    fn an_exact_declaration_wins_over_a_parameter_stripped_one() {
        let declared = content(&["text/plain; charset=iso-8859-1", "text/plain"]);

        let chosen = super::declared_representation(&declared, "text/plain");
        assert!(chosen.is_some());
        assert!(std::ptr::eq(
            chosen.expect("a representation"),
            declared.get("text/plain").expect("the exact entry"),
        ));
    }

    /// The control: relaxing the parameter comparison must not make an
    /// unrelated media type match.
    #[test]
    fn an_undeclared_media_type_still_does_not_match() {
        let declared = content(&["text/html; charset=utf-8"]);

        assert!(super::declared_representation(&declared, "application/json").is_none());
    }
}
