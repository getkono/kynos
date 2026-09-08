use crate::{
    Map,
    model::{
        body::media_type::MediaType,
        parameter::header::Header,
        reference::RefOr,
        response::{Response, Responses, status::StatusPattern},
        schema::{Schema, object::SchemaObject},
    },
};

const WILDCARDS: &[StatusPattern] = &[
    StatusPattern::Informational,
    StatusPattern::Success,
    StatusPattern::Redirection,
    StatusPattern::ClientError,
    StatusPattern::ServerError,
];

/// The wire spelling of each wildcard, transcribed from the specification.
///
/// An exhaustive match, so a wildcard added to [`StatusPattern`] stops this
/// file compiling until its spelling is written down. `Code` is excluded
/// because it spells itself.
///
/// This is the oracle, and it exists because the round trip it replaces was
/// not one: parsing `1XX` and rendering it back consults `FromStr` and
/// `Display`, which are the same table written twice. Two spellings swapped
/// between two variants satisfied that check in both directions.
fn spelling(pattern: StatusPattern) -> Option<&'static str> {
    match pattern {
        StatusPattern::Informational => Some("1XX"),
        StatusPattern::Success => Some("2XX"),
        StatusPattern::Redirection => Some("3XX"),
        StatusPattern::ClientError => Some("4XX"),
        StatusPattern::ServerError => Some("5XX"),
        StatusPattern::Code(_) => None,
    }
}

#[test]
fn each_wildcard_parses_and_renders_as_the_specification_spells_it() {
    for &pattern in WILDCARDS {
        let spelled = spelling(pattern).expect("a wildcard has a spelling");

        assert_eq!(
            spelled.parse::<StatusPattern>().expect("a legal key"),
            pattern,
            "parsing {spelled}"
        );
        assert_eq!(pattern.to_string(), spelled, "rendering {pattern:?}");
    }
}

/// Every status a response can carry, and the bounds on either side of them.
#[test]
fn an_exact_code_parses_and_renders_as_itself() {
    for code in 100..=599u16 {
        let text = code.to_string();

        assert_eq!(
            text.parse::<StatusPattern>().expect("a status in range"),
            StatusPattern::Code(code)
        );
        assert_eq!(StatusPattern::Code(code).to_string(), text);
    }

    for code in [0u16, 99, 600, 999] {
        assert!(
            code.to_string().parse::<StatusPattern>().is_err(),
            "{code} is not a status a response can carry"
        );
    }
}

/// The wildcards are five, and nothing shaped like a sixth is one.
#[test]
fn only_the_five_documented_wildcards_are_accepted() {
    assert!("6XX".parse::<StatusPattern>().is_err());
    assert!("2xx".parse::<StatusPattern>().is_err());
    assert!("20X".parse::<StatusPattern>().is_err());
    assert!("XX".parse::<StatusPattern>().is_err());
}

/// Each wildcard and the class it covers.
///
/// An exhaustive match, so a wildcard added to [`StatusPattern`] stops this
/// file compiling until its range is written down. `Code` is excluded because
/// it is not a class -- it is checked separately below.
fn covered_class(pattern: StatusPattern) -> Option<std::ops::RangeInclusive<u16>> {
    match pattern {
        StatusPattern::Informational => Some(100..=199),
        StatusPattern::Success => Some(200..=299),
        StatusPattern::Redirection => Some(300..=399),
        StatusPattern::ClientError => Some(400..=499),
        StatusPattern::ServerError => Some(500..=599),
        StatusPattern::Code(_) => None,
    }
}

#[test]
fn wildcards_cover_their_class_and_nothing_else() {
    for &pattern in WILDCARDS {
        let class = covered_class(pattern).expect("a wildcard covers a class");
        // Every status a response can carry, against every wildcard: the
        // boundaries are where an off-by-one would hide.
        for code in 100..=599u16 {
            assert_eq!(
                pattern.matches(code),
                class.contains(&code),
                "{pattern:?} against {code}"
            );
        }
    }
}

#[test]
fn an_exact_code_matches_only_itself() {
    for code in [200u16, 404, 500] {
        let pattern = StatusPattern::Code(code);
        assert!(covered_class(pattern).is_none());
        for other in 100..=599u16 {
            assert_eq!(
                pattern.matches(other),
                other == code,
                "{code} against {other}"
            );
        }
    }
}

#[test]
fn responses_serialize_default_alongside_status_keys() {
    let responses = Responses::new()
        .with(200, Response::new("ok"))
        .with_default(Response::new("unexpected error"));
    let json = serde_json::to_string(&responses).expect("ok");
    assert!(json.contains(r#""default""#));
    assert!(json.contains(r#""200""#));
}

#[test]
fn a_malformed_status_key_is_a_parse_error() {
    let result = serde_json::from_str::<Responses>(r#"{"okay":{"description":"x"}}"#);
    assert!(result.is_err());
}

#[test]
fn extensions_survive_a_round_trip() {
    let parsed: Responses =
        serde_json::from_str(r#"{"200":{"description":"ok"},"x-note":"hi"}"#).expect("ok");
    assert_eq!(
        parsed.extensions.get("x-note").and_then(|v| v.as_str()),
        Some("hi")
    );
    assert_eq!(parsed.responses.len(), 1);
}

#[test]
fn merging_keeps_the_existing_entry_on_conflict() {
    let mut base = Responses::new().with(200, Response::new("mine"));
    let other = Responses::new()
        .with(200, Response::new("theirs"))
        .with(429, Response::new("too many requests"));
    base.merge_from(&other);

    assert_eq!(base.responses.len(), 2);
    let two_hundred = base.get(200).and_then(RefOr::as_item).expect("present");
    assert_eq!(two_hundred.description.as_deref(), Some("mine"));
}

/// A 3.2 Response Object stating only a summary parses.
///
/// 3.1 marks `description` **REQUIRED** (`references/3.1.2.md:2010`). 3.2 drops
/// the marker (`references/3.2.0.md:2161`), and its meta-schema's
/// `$defs/response` carries no `required` array at all — so a response with a
/// `summary` and nothing else is a legal 3.2 document. `description: String`
/// makes it unparseable, which is the model refusing to read something the
/// specification allows.
///
/// The requirement does not go away; it moves to where it is true. 3.1 still
/// demands one, and `validate` is what says so.
#[cfg(feature = "openapi32")]
#[test]
fn a_response_stating_only_a_summary_parses() {
    let parsed: Response =
        serde_json::from_str(r#"{"summary":"The order"}"#).expect("a legal 3.2 Response Object");

    assert_eq!(parsed.summary.as_deref(), Some("The order"));
    assert_eq!(
        serde_json::to_string(&parsed).expect("serializable"),
        r#"{"summary":"The order"}"#,
        "and writes back what it read, without inventing a description"
    );
}

// --- Two problem responses meeting on one status -----------------------------
//
// `union_from` is `merge_from` plus one exception, so what is asserted here is
// the exception and the boundary of it: the shapes it applies to, the shape it
// produces, and the two ways of not being that shape.

/// The shared component every problem response refers to.
fn component() -> Schema {
    Schema::component("Problem")
}

/// One narrowed branch: the shared component, and the type it publishes.
fn branch(uri: &str) -> Schema {
    let mut properties = Map::new();
    properties.insert(
        "type".to_owned(),
        Schema::Object(Box::new(SchemaObject {
            const_value: Some(serde_json::Value::String(uri.to_owned())),
            ..SchemaObject::default()
        })),
    );

    Schema::Object(Box::new(SchemaObject {
        all_of: Some(vec![
            component(),
            Schema::Object(Box::new(SchemaObject {
                properties,
                ..SchemaObject::default()
            })),
        ]),
        ..SchemaObject::default()
    }))
}

/// A schema carrying `one_of` and nothing else.
fn choice(branches: Vec<Schema>) -> Schema {
    Schema::Object(Box::new(SchemaObject {
        one_of: Some(branches),
        ..SchemaObject::default()
    }))
}

/// A problem response narrowing `type` to one URI, as the derive emits it.
fn narrowed(description: &str, uri: &str) -> Response {
    problem(description, branch(uri))
}

/// A problem response referring to the shared component and narrowing nothing.
fn unnarrowed(description: &str) -> Response {
    problem(description, component())
}

fn problem(description: &str, schema: Schema) -> Response {
    Response::with_content(
        description,
        "application/problem+json",
        MediaType::new(schema),
    )
}

/// The declared problem schema for `status`, as JSON.
fn declared(responses: &Responses, status: u16) -> serde_json::Value {
    let response = responses
        .get(status)
        .and_then(RefOr::as_item)
        .expect("a declared status");

    serde_json::to_value(&response.content["application/problem+json"].schema)
        .expect("a schema serializes")
}

/// Every URI a branch of `schema` constrains `type` to.
fn published(schema: &serde_json::Value) -> Vec<String> {
    let branches = schema["oneOf"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![schema.clone()]);

    branches
        .iter()
        .filter_map(|branch| branch["allOf"][1]["properties"]["type"]["const"].as_str())
        .map(ToOwned::to_owned)
        .collect()
}

/// Two narrowed sides are a choice between what each publishes, and a status
/// only one side names still arrives.
#[test]
fn a_status_two_narrowed_problems_share_publishes_both_of_them() {
    let mut base = Responses::new().with(400, narrowed("Bad Request", "about:blank"));
    let other = Responses::new()
        .with(
            400,
            narrowed("Empty review", "https://errors.example.test/empty"),
        )
        .with(409, narrowed("Taken", "https://errors.example.test/taken"));
    base.union_from(&other);

    assert_eq!(
        published(&declared(&base, 400)),
        vec![
            "about:blank".to_owned(),
            "https://errors.example.test/empty".to_owned()
        ]
    );
    assert!(
        base.get(409).is_some(),
        "a status only one side names arrives"
    );
}

/// The description says what each side meant, and says it once.
#[test]
fn a_unioned_status_describes_both_sides_without_repeating_one() {
    let mut base = Responses::new().with(400, narrowed("Bad Request", "about:blank"));
    let other = Responses::new().with(
        400,
        narrowed(
            "Bad Request; Empty review",
            "https://errors.example.test/empty",
        ),
    );
    base.union_from(&other);

    assert_eq!(
        base.get(400)
            .and_then(RefOr::as_item)
            .and_then(|response| response.description.as_deref()),
        Some("Bad Request; Empty review")
    );
}

/// Two sides publishing one URI are one branch. A `oneOf` repeating a `const`
/// is satisfied by two branches at once, which is what the keyword forbids —
/// so the dedup is what keeps the union sound rather than merely shorter.
#[test]
fn two_sides_publishing_one_type_declare_one_branch() {
    let mut base = Responses::new().with(400, narrowed("Bad Request", "about:blank"));
    let other = Responses::new().with(400, narrowed("Malformed body", "about:blank"));
    base.union_from(&other);

    let schema = declared(&base, 400);
    assert_eq!(schema.get("oneOf"), None, "{schema}");
    assert_eq!(published(&schema), vec!["about:blank".to_owned()]);
}

/// A bare `$ref` matches every problem document, so it is already the union —
/// whichever side carries it. Narrowing to the other side would declare less
/// than the operation can send.
#[test]
fn an_unnarrowed_side_is_the_union_from_either_direction() {
    let narrow = || Responses::new().with(403, narrowed("Forbidden", "about:blank"));
    let wide = || Responses::new().with(403, unnarrowed("Forbidden"));

    let mut declared_narrow = narrow();
    declared_narrow.union_from(&wide());

    let mut declared_wide = wide();
    declared_wide.union_from(&narrow());

    assert_eq!(declared(&declared_narrow, 403), declared(&wide(), 403));
    assert_eq!(declared(&declared_wide, 403), declared(&wide(), 403));
}

/// Everything the declared entry carries beyond its schema survives a
/// contributor arriving after it. A 401 gains its `WWW-Authenticate` from the
/// scheme rather than from the rejection, so a union that rebuilt the response
/// would drop the one field RFC 9110 section 11.6.1 requires.
#[test]
fn a_union_keeps_what_the_declared_entry_carries() {
    let mut base = Responses::new().with(
        401,
        narrowed("Unauthorized", "about:blank")
            .with_header("WWW-Authenticate", Header::new(Schema::any())),
    );
    base.union_from(&Responses::new().with(
        401,
        narrowed("Expired", "https://errors.example.test/expired"),
    ));

    let response = base
        .get(401)
        .and_then(RefOr::as_item)
        .expect("a declared 401");
    assert!(response.headers.contains_key("WWW-Authenticate"));
}

/// Anything that is not two problem documents keeps the entry already
/// declared, which is `merge_from`'s rule and the reason this is a second
/// method rather than a change to that one.
#[test]
fn two_responses_that_are_not_problems_keep_the_one_declared() {
    let mut base = Responses::new().with(
        200,
        Response::with_content("mine", "application/json", MediaType::new(component())),
    );
    base.union_from(&Responses::new().with(
        200,
        Response::with_content("theirs", "application/json", MediaType::new(component())),
    ));

    assert_eq!(
        base.get(200)
            .and_then(RefOr::as_item)
            .and_then(|response| response.description.as_deref()),
        Some("mine")
    );
}

// --- Shapes the union may not adopt -----------------------------------------
//
// `union_from` adopts one side outright where that side admits every problem
// document the other describes. Three shapes are read by none of its rules and
// admit strictly *less* than a narrowed side, so adopting one would declare
// less than the operation sends -- the direction the union exists to prevent.
// Each is asserted separately, because each reaches the decision by a different
// route.

/// A schema satisfied by nothing admits nothing.
///
/// Adopting it would leave the operation declaring `false` for a status it
/// answers, so a real body fails the schema its own description gave it.
#[test]
fn a_side_admitting_nothing_is_not_the_union() {
    let mut base = Responses::new().with(
        400,
        narrowed("Bad Request", "https://errors.example.test/a"),
    );
    base.union_from(&Responses::new().with(400, problem("nothing at all", Schema::never())));

    assert_eq!(
        published(&declared(&base, 400)),
        vec!["https://errors.example.test/a".to_owned()],
        "the declared narrowing survived a side that admits nothing"
    );
}

/// An empty `oneOf` is satisfied by no branch, so it is the same claim as
/// `false` written another way.
#[test]
fn a_side_choosing_between_nothing_is_not_the_union() {
    let mut base = Responses::new().with(
        400,
        narrowed("Bad Request", "https://errors.example.test/a"),
    );
    base.union_from(&Responses::new().with(400, problem("no branch", choice(Vec::new()))));

    assert_eq!(
        published(&declared(&base, 400)),
        vec!["https://errors.example.test/a".to_owned()]
    );
}

/// A `oneOf` mixing a bare `$ref` with a narrowed branch is satisfied by
/// *neither* for a document the narrowed branch describes: the body matches
/// both branches, and `oneOf` requires exactly one. It admits less than either
/// side alone, so it is not the union of them.
#[test]
fn a_side_whose_branches_overlap_is_not_the_union() {
    let mut base = Responses::new().with(
        400,
        narrowed("Bad Request", "https://errors.example.test/a"),
    );
    base.union_from(&Responses::new().with(
        400,
        problem(
            "either",
            choice(vec![component(), branch("https://errors.example.test/b")]),
        ),
    ));

    assert_eq!(
        published(&declared(&base, 400)),
        vec!["https://errors.example.test/a".to_owned()]
    );
}

/// The other half of the same split: `true` admits everything, so it *is* the
/// union, exactly as a bare `$ref` is.
#[test]
fn a_side_admitting_everything_is_the_union() {
    let mut base = Responses::new().with(
        400,
        narrowed("Bad Request", "https://errors.example.test/a"),
    );
    base.union_from(&Responses::new().with(400, problem("anything", Schema::any())));

    assert_eq!(
        declared(&base, 400),
        serde_json::json!(true),
        "a side admitting every document is what the status declares"
    );
}

/// A side already holding a `oneOf` contributes its branches, not itself.
///
/// The case a derive with two variants on one status reaches, and the one a
/// union that nested rather than flattened would break: `oneOf` of a `oneOf`
/// is satisfied by exactly one *outer* branch, so a document matching one
/// inner branch of a two-branch side satisfies the outer branch it sits in and
/// the whole is still one — but nothing then holds the inner choice to being
/// exclusive of the branch beside it.
#[test]
fn a_side_already_choosing_contributes_its_branches() {
    let mut base = Responses::new().with(
        400,
        problem(
            "Empty; Too long",
            choice(vec![
                branch("https://errors.example.test/empty"),
                branch("https://errors.example.test/too-long"),
            ]),
        ),
    );
    base.union_from(&Responses::new().with(400, narrowed("Bad Request", "about:blank")));

    let schema = declared(&base, 400);
    assert_eq!(
        published(&schema),
        vec![
            "https://errors.example.test/empty".to_owned(),
            "https://errors.example.test/too-long".to_owned(),
            "about:blank".to_owned(),
        ],
        "{schema}"
    );
    assert!(
        schema["oneOf"]
            .as_array()
            .expect("three branches are a choice")
            .iter()
            .all(|branch| branch.get("oneOf").is_none()),
        "a branch is a branch rather than a nested choice: {schema}"
    );
}
