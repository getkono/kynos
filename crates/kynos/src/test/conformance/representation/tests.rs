use super::declared_representation;
use crate::test::conformance::tests::content;

/// A `Content-Type` is compared with its parameters already stripped, so a
/// declared media type carrying one has to match the bare form -- or every
/// parameterized entry in a document reads as undeclared. `text/html` and
/// `text/css` both reach the document that way through `router::assets`.
#[test]
fn a_declared_media_type_matches_despite_its_parameters() {
    let declared = content(&["text/html; charset=utf-8"]);

    assert!(declared_representation(&declared, "text/html").is_some());
}

/// The exact match is tried first, so two representations differing only by
/// parameter resolve to the one actually named rather than to whichever the
/// map happened to hold first.
#[test]
fn an_exact_declaration_wins_over_a_parameter_stripped_one() {
    let declared = content(&["text/plain; charset=iso-8859-1", "text/plain"]);

    let chosen = declared_representation(&declared, "text/plain");
    assert!(chosen.is_some());
    assert!(std::ptr::eq(
        chosen.expect("a representation").1,
        declared.get("text/plain").expect("the exact entry"),
    ));
}

/// The control: relaxing the parameter comparison must not make an
/// unrelated media type match.
#[test]
fn an_undeclared_media_type_still_does_not_match() {
    let declared = content(&["text/html; charset=utf-8"]);

    assert!(declared_representation(&declared, "application/json").is_none());
}

/// A `content` key may be a media range (OAS 3.1.1 `response-content`,
/// RFC 9110 section 12.5.1), parameters and all, so a range covers every
/// media type it names.
#[test]
fn a_media_range_key_declares_every_media_type_it_covers() {
    for (range, media_type) in [
        ("image/*", "image/png"),
        ("IMAGE/*", "image/png"),
        ("text/*; charset=utf-8", "text/plain"),
        ("*/*", "application/json"),
    ] {
        let declared = content(&[range]);
        assert!(
            declared_representation(&declared, media_type).is_some(),
            "{range} should cover {media_type}"
        );
    }
}

/// A range covers its own type only: `image/*` says nothing about `text/plain`,
/// and a range's `*` is never a prefix match on the type.
#[test]
fn a_media_range_key_does_not_cover_another_type() {
    for (range, media_type) in [
        ("image/*", "text/plain"),
        ("image/*", "imagery/png"),
        ("text/*", "application/text"),
    ] {
        let declared = content(&[range]);
        assert!(
            declared_representation(&declared, media_type).is_none(),
            "{range} should not cover {media_type}"
        );
    }
}

/// A range covers media types, so a `Content-Type` not shaped `type/subtype`
/// (RFC 9110 section 8.3.1) is covered by none, not even `*/*`.
#[test]
fn a_media_range_key_does_not_cover_a_value_that_is_not_a_media_type() {
    for range in ["*/*", "text/*"] {
        for media_type in ["", "garbage", "text", "text/", "/plain", "/"] {
            let declared = content(&[range]);
            assert!(
                declared_representation(&declared, media_type).is_none(),
                "{range} should not cover {media_type:?}"
            );
        }
    }
}

/// Only the most specific matching key applies (`text/plain` overrides
/// `text/*`), whatever order the map holds them in.
#[test]
fn the_most_specific_matching_key_applies() {
    for order in [
        ["*/*", "image/*", "image/png"],
        ["image/png", "image/*", "*/*"],
    ] {
        let declared = content(&order);
        for (media_type, expected) in [
            ("image/png", "image/png"),
            ("image/gif", "image/*"),
            ("text/plain", "*/*"),
        ] {
            let chosen = declared_representation(&declared, media_type);
            assert_eq!(
                chosen.map(|(key, _)| key),
                Some(expected),
                "{order:?}: {media_type} should resolve to {expected}"
            );
        }
    }
}

/// Keys equally specific resolve to the first the map holds, so the choice
/// stays deterministic whichever of them is declared first.
#[test]
fn the_first_of_equally_specific_keys_applies() {
    for order in [
        ["text/*", "text/*; charset=utf-8"],
        ["text/*; charset=utf-8", "text/*"],
    ] {
        let declared = content(&order);
        let chosen = declared_representation(&declared, "text/plain");
        assert_eq!(
            chosen.map(|(key, _)| key),
            Some(order[0]),
            "{order:?}: text/plain should resolve to {}",
            order[0]
        );
    }
}
