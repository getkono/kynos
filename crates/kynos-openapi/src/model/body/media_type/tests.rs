use serde_json::json;

use super::{Example, Examples, MediaType};

/// The whole table, transcribed.
///
/// A closed enumeration under `docs/testing.md`: the list is the claim, so
/// a row added or removed without a reader noticing fails here rather than
/// being sampled around. Every entry is a media type OpenAPI 3.2 gives
/// `itemSchema` for, and there is no membership rule to derive one from —
/// the specification names them.
#[cfg(feature = "openapi32")]
#[test]
fn the_sequential_media_type_table_is_closed() {
    assert_eq!(
        super::SEQUENTIAL_MEDIA_TYPES,
        [
            "application/jsonl",
            "application/x-ndjson",
            "application/json-seq",
            "application/geo+json-seq",
            "text/event-stream",
            "multipart/mixed",
            "multipart/byteranges",
        ]
    );
}

/// No entry is listed twice, and each is a media type.
#[cfg(feature = "openapi32")]
#[test]
fn every_row_is_a_media_type_named_once() {
    let mut seen: Vec<&str> = super::SEQUENTIAL_MEDIA_TYPES.to_vec();
    let before = seen.len();
    seen.sort_unstable();
    seen.dedup();

    assert_eq!(seen.len(), before, "a media type is listed more than once");
    for media_type in super::SEQUENTIAL_MEDIA_TYPES {
        assert!(
            media_type.contains('/') && *media_type == media_type.to_ascii_lowercase(),
            "`{media_type}` is not a media type"
        );
    }
}

#[test]
fn a_body_shown_both_ways_at_once_is_refused() {
    let error =
        serde_json::from_str::<MediaType>(r#"{"example":1,"examples":{"one":{"value":1}}}"#)
            .expect_err("`example` is exclusive with `examples`");

    assert!(error.to_string().contains("mutually exclusive"));
}

#[test]
fn a_named_example_replaces_an_inline_one() {
    let media_type = MediaType::default()
        .with_example(json!("inline"))
        .with_named_example("one", Example::new(1));

    assert!(media_type.example().is_none());
    assert!(matches!(media_type.examples(), Some(Examples::Named(named)) if named.len() == 1));
}

#[test]
fn an_inline_example_replaces_named_ones() {
    let media_type = MediaType::default()
        .with_named_example("one", Example::new(1))
        .with_example(json!("inline"));

    assert!(media_type.named_examples().is_none());
    assert_eq!(media_type.example(), Some(&json!("inline")));
}
