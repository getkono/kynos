use crate::model::{
    reference::RefOr,
    response::{Response, Responses, status::StatusPattern},
};

#[test]
fn status_patterns_round_trip_through_strings() {
    for text in ["200", "404", "1XX", "2XX", "3XX", "4XX", "5XX"] {
        let pattern: StatusPattern = text.parse().expect("valid");
        assert_eq!(pattern.to_string(), text);
    }
}

#[test]
fn only_the_five_documented_wildcards_are_accepted() {
    assert!("6XX".parse::<StatusPattern>().is_err());
    assert!("2xx".parse::<StatusPattern>().is_err());
    assert!("20X".parse::<StatusPattern>().is_err());
    assert!("99".parse::<StatusPattern>().is_err());
    assert!("600".parse::<StatusPattern>().is_err());
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
    const WILDCARDS: &[StatusPattern] = &[
        StatusPattern::Informational,
        StatusPattern::Success,
        StatusPattern::Redirection,
        StatusPattern::ClientError,
        StatusPattern::ServerError,
    ];

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
    assert_eq!(two_hundred.description, "mine");
}
