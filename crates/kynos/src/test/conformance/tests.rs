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
        document.paths.items.insert(
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
