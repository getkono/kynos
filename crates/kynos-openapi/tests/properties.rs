//! Properties over a whole generated document.
//!
//! Round-tripping, determinism, the totality of validation, and the two
//! emission rules; beside them, the totality of `pattern::translate`, whose
//! input is a Schema Object's `pattern`. The path-template properties are in
//! `templates.rs`; the generators both files draw on are in `support/`.

use kynos_openapi::{
    Document, Info, Method, Operation, PathItem, PathTemplate, RefOr, Response, Responses,
    Severity, SpecError, SpecVersion, Violation,
    annotation::{OPAQUE_OPERATION_ANNOTATION, OPAQUE_ROUTES_ANNOTATION},
    emit::downgrade::three_two_only_constructs,
    validate::Validator,
};
use proptest::prelude::*;

#[path = "support/mod.rs"]
mod support;
use support::*;

// --- Helpers used by the properties -------------------------------------

fn to_json(document: &Document) -> String {
    document
        .to_json()
        .expect("every generated value is representable in JSON")
}

fn parse(json: &str) -> Document {
    serde_json::from_str(json).expect("what the model emits, the model reads")
}

/// Violations as a sorted multiset of their rendered form.
///
/// Two rules collect their inputs through a `HashSet`, so the order of the
/// violations they emit is not fixed; the set of them is what the caller acts
/// on.
fn rendered(violations: &[Violation]) -> Vec<String> {
    let mut rendered: Vec<String> = violations.iter().map(ToString::to_string).collect();
    rendered.sort();
    rendered
}

/// The rendered form of the error-severity violations only.
fn rendered_errors(violations: &[Violation]) -> Vec<String> {
    let mut rendered: Vec<String> = violations
        .iter()
        .filter(|violation| violation.severity == Severity::Error)
        .map(ToString::to_string)
        .collect();
    rendered.sort();
    rendered
}

/// Every place the emitted JSON holds an unchecked schema, as sorted pointers.
///
/// Read off the wire rather than the model, so the oracle shares no walk with
/// the validator: an object carrying `x-kynos-unchecked` anywhere, and `true`
/// wherever it is a Media Type Object's own `schema` or `itemSchema` — under a
/// `content` map, or as a 3.2 `components.mediaTypes` entry.
fn unchecked_pointers(document: &Document) -> Vec<String> {
    fn is_media_type_schema(tokens: &[String]) -> bool {
        match tokens {
            [.., parent, _, last] if parent == "content" => {
                last == "schema" || last == "itemSchema"
            }
            [components, media_types, _, last]
                if components == "components" && media_types == "mediaTypes" =>
            {
                last == "schema" || last == "itemSchema"
            }
            _ => false,
        }
    }

    fn walk(value: &serde_json::Value, tokens: &mut Vec<String>, found: &mut Vec<String>) {
        let pointer = |tokens: &[String]| {
            tokens.iter().fold("#".to_owned(), |pointer, token| {
                format!("{pointer}/{token}")
            })
        };
        match value {
            serde_json::Value::Object(map) => {
                if map.contains_key("x-kynos-unchecked") {
                    found.push(pointer(tokens));
                }
                for (key, child) in map {
                    tokens.push(key.replace('~', "~0").replace('/', "~1"));
                    walk(child, tokens, found);
                    tokens.pop();
                }
            }
            serde_json::Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    tokens.push(index.to_string());
                    walk(child, tokens, found);
                    tokens.pop();
                }
            }
            serde_json::Value::Bool(true) if is_media_type_schema(tokens) => {
                found.push(pointer(tokens));
            }
            _ => {}
        }
    }

    let json = serde_json::to_value(document).expect("every generated value is representable");
    let mut found = Vec::new();
    walk(&json, &mut Vec::new(), &mut found);
    found.sort();
    found
}

/// Every operation the emitted JSON marks opaque, as sorted pointers, each
/// with the reasons its marker records or `None` where the marker is not in
/// the shape Kynos emits.
///
/// Read off the wire, as `unchecked_pointers` is, so the oracle shares no
/// walk with the validator or with `Document::is_authoritative`: the
/// generators plant `x-kynos-opaque` on operations and nowhere else, so every
/// object carrying it is one. A marker is well formed when it is an object
/// whose `reasons` is an array of strings and whose `note`, if any, is a
/// string.
fn opaque_markers(document: &Document) -> Vec<(String, Option<Vec<String>>)> {
    fn read(marker: &serde_json::Value) -> Option<Vec<String>> {
        let marker = marker.as_object()?;
        if marker.get("note").is_some_and(|note| !note.is_string()) {
            return None;
        }
        marker
            .get("reasons")?
            .as_array()?
            .iter()
            .map(|reason| reason.as_str().map(str::to_owned))
            .collect()
    }

    fn walk(
        value: &serde_json::Value,
        pointer: &str,
        found: &mut Vec<(String, Option<Vec<String>>)>,
    ) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(marker) = map.get(OPAQUE_OPERATION_ANNOTATION) {
                    found.push((pointer.to_owned(), read(marker)));
                }
                for (key, child) in map {
                    let token = key.replace('~', "~0").replace('/', "~1");
                    walk(child, &format!("{pointer}/{token}"), found);
                }
            }
            serde_json::Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    walk(child, &format!("{pointer}/{index}"), found);
                }
            }
            _ => {}
        }
    }

    let json = serde_json::to_value(document).expect("every generated value is representable");
    let mut found = Vec::new();
    walk(&json, "#", &mut found);
    found.sort();
    found
}

// --- Properties ----------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Parsing what was emitted yields the document that was emitted.
    #[test]
    fn a_document_survives_a_json_round_trip(document in arb_document()) {
        prop_assert_eq!(parse(&to_json(&document)), document);
    }

    /// Serialization depends on nothing but the value.
    #[test]
    fn serialization_is_deterministic(document in arb_document()) {
        let json = to_json(&document);
        prop_assert_eq!(to_json(&document.clone()), json.clone());
        prop_assert_eq!(to_json(&parse(&json)), json);
    }

    /// YAML emission writes exactly what `serde_norway` writes for the model.
    ///
    /// `to_yaml` may take any route to its output, but in a build where
    /// `serde_json`'s numbers serialize as numbers that output is the model's
    /// own YAML, byte for byte. A route that reorders a key, restyles a scalar
    /// or drops a tag fails here rather than in a downstream diff.
    #[cfg(feature = "yaml")]
    #[test]
    fn yaml_emission_is_what_serde_norway_writes_for_the_model(document in arb_document()) {
        prop_assert_eq!(
            document.to_yaml().expect("every generated value is representable in YAML"),
            serde_norway::to_string(&document).expect("the model serializes to YAML")
        );
    }

    /// Validation terminates and reports the same thing every time, at every
    /// specification version, for any document at all.
    #[test]
    fn validation_is_total(document in arb_document()) {
        for &version in VERSIONS {
            let validator = Validator::new(version);
            let violations = validator.validate(&document);
            prop_assert_eq!(rendered(&validator.validate(&document)), rendered(&violations));

            let errors = document.validate(version).err().unwrap_or_default();
            prop_assert_eq!(rendered(&errors), rendered_errors(&violations));

            for violation in &violations {
                prop_assert!(violation.location.starts_with('#'));
            }
        }
    }

    /// Every unchecked schema a document holds is reported once, at its own
    /// pointer, whatever container it sits in and however deep.
    #[test]
    fn every_unchecked_schema_is_reported_once_where_it_sits(document in arb_document()) {
        let expected = unchecked_pointers(&document);
        for &version in VERSIONS {
            let mut reported: Vec<String> = Validator::new(version)
                .validate(&document)
                .into_iter()
                .filter(|violation| violation.error == SpecError::UncheckedSchema)
                .map(|violation| violation.location)
                .collect();
            reported.sort();
            prop_assert_eq!(&reported, &expected);
        }
    }

    /// Every opaque operation a document holds is reported once, at its own
    /// pointer, whatever path item, webhook, component or callback it sits
    /// in: as a warning carrying its reasons, or as an error where its marker
    /// is unreadable. The document is authoritative exactly when no operation
    /// is marked and no route is recorded.
    #[test]
    fn every_opaque_operation_is_reported_once_where_it_sits(document in arb_document()) {
        let markers = opaque_markers(&document);
        let expected: Vec<_> = markers
            .iter()
            .map(|(pointer, reasons)| {
                let severity = if reasons.is_some() { Severity::Warning } else { Severity::Error };
                (pointer.clone(), severity, reasons.clone())
            })
            .collect();
        for &version in VERSIONS {
            let mut reported: Vec<_> = Validator::new(version)
                .validate(&document)
                .into_iter()
                .filter_map(|violation| match violation.error {
                    SpecError::OpaqueOperation { reasons } => Some((
                        violation.location,
                        violation.severity,
                        Some(reasons.iter().map(|reason| reason.as_str().to_owned()).collect()),
                    )),
                    SpecError::MalformedAnnotation { name, .. }
                        if name == OPAQUE_OPERATION_ANNOTATION =>
                    {
                        Some((violation.location, violation.severity, None))
                    }
                    _ => None,
                })
                .collect();
            reported.sort();
            prop_assert_eq!(&reported, &expected);
        }

        // `x-kynos-opaque-routes` reads as no route only when absent or empty.
        let no_routes = document
            .extensions
            .get(OPAQUE_ROUTES_ANNOTATION)
            .is_none_or(|routes| *routes == serde_json::json!([]));
        prop_assert_eq!(document.is_authoritative(), markers.is_empty() && no_routes);
    }

    /// A document the validator accepts is one the model reads back, whatever
    /// key its responses are written under: `parse ∘ emit = id` on every
    /// accepted response key, and a refusal on every other.
    ///
    /// `arb_responses` draws only legal keys so the round trip above holds;
    /// this draws the codes `Responses::with` takes and the strings the public
    /// map holds.
    #[test]
    fn a_response_key_is_accepted_exactly_when_it_reads_back(
        key in prop_oneof![
            any::<u16>().prop_map(|status| status.to_string()),
            "[0-9xX+-]{0,4}",
            Just("default".to_owned()),
            Just("x-a".to_owned()),
        ]
    ) {
        let mut responses = Responses::new();
        responses.responses.insert(key, RefOr::Item(Response::new("ok")));
        let mut document = Document::new(SpecVersion::V3_1, Info::new("Test", "1.0.0"));
        document.paths.insert(
            &PathTemplate::parse("/users").expect("a legal template"),
            PathItem::new().with_operation(
                Method::Get,
                Operation::new("listUsers").with_responses(responses),
            ),
        );

        let reads_back = serde_json::from_str::<Document>(&to_json(&document))
            .is_ok_and(|parsed| parsed == document);
        for &version in VERSIONS {
            prop_assert_eq!(document.validate(version).is_ok(), reads_back);
        }
    }

    /// Emitting as 3.1 fails exactly when a 3.2-only construct is in the way,
    /// and emitting at a fixed version is idempotent.
    #[test]
    fn emitting_refuses_a_lossy_downgrade(document in arb_document()) {
        let blockers = three_two_only_constructs(&document);
        match document.emit(SpecVersion::V3_1) {
            Ok(emitted) => {
                prop_assert!(blockers.is_empty());
                prop_assert_eq!(&emitted.openapi, "3.1.2");
                prop_assert_eq!(emitted.emit(SpecVersion::V3_1).ok(), Some(emitted.clone()));
                prop_assert_eq!(three_two_only_constructs(&emitted), blockers);
            }
            Err(error) => {
                prop_assert!(!blockers.is_empty());
                prop_assert_eq!(error, SpecError::RequiresV3_2 { blockers });
            }
        }
    }

    /// A document is emittable as the version it already declares.
    #[cfg(feature = "openapi32")]
    #[test]
    fn emitting_as_the_newer_version_always_succeeds(document in arb_document()) {
        let emitted = document.emit(SpecVersion::V3_2).expect("3.2 expresses everything");
        prop_assert_eq!(&emitted.openapi, "3.2.0");
        prop_assert_eq!(emitted.emit(SpecVersion::V3_2).ok(), Some(emitted.clone()));
        prop_assert_eq!(emitted.spec_version(), Some(SpecVersion::V3_2));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    /// `translate` answers every string without panicking, and every
    /// translation it returns is one the `regex` engine compiles, as its
    /// documentation promises.
    #[cfg(feature = "pattern")]
    #[test]
    fn translating_a_pattern_is_total(source in arb_pattern_source()) {
        if let Ok(translated) = kynos_openapi::pattern::translate(&source) {
            prop_assert!(
                regex::Regex::new(&translated).is_ok(),
                "`{}` translated to `{}`, which does not compile",
                source,
                translated
            );
        }
    }
}
