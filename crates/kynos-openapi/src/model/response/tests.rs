use crate::model::{
    reference::RefOr,
    response::{Response, Responses, status::StatusPattern},
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
fn responses_round_trip() {
    let responses = Responses::new().with(201, Response::new("created"));
    let json = serde_json::to_string(&responses).expect("ok");
    let parsed: Responses = serde_json::from_str(&json).expect("ok");
    assert_eq!(parsed, responses);
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
