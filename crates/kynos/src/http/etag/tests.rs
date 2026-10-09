use super::{ANY, if_match, matches, matches_strongly, split, strong_match, weak_match};
use crate::http::{HeaderMap, HeaderValue, header};

/// A list assembled from members, and the members it was assembled from.
///
/// The independently constructed oracle `docs/testing.md` asks a parser
/// for: it carries the tags recorded while the string was being built, so
/// the property compares [`split`] against something that never consulted
/// it. `TemplateCase` in `kynos-openapi`'s `tests/support/` is the shape.
struct ListCase {
    written: String,
    members: Vec<String>,
}

/// Every list of up to three tags drawn from a small alphabet, written with
/// each of the separators RFC 9110 section 5.6.1 admits.
///
/// A sweep rather than a draw: the space closes, and `docs/testing.md`
/// reads the parser rule as asking for an independent oracle rather than
/// for `proptest` specifically.
fn every_list() -> Vec<ListCase> {
    // Each of these is one tag, including the two whose `opaque-tag`
    // carries the separator character.
    let alphabet = [r#""a""#, r#""a,b""#, r#"W/"c""#, r#""d,""#, r#""""#];
    let separators = [",", ", ", " ,", " , ", ",\t"];

    let mut cases = Vec::new();
    for separator in separators {
        for first in alphabet {
            cases.push(ListCase {
                written: first.to_owned(),
                members: vec![first.to_owned()],
            });

            for second in alphabet {
                cases.push(ListCase {
                    written: format!("{first}{separator}{second}"),
                    members: vec![first.to_owned(), second.to_owned()],
                });

                for third in alphabet {
                    cases.push(ListCase {
                        written: format!("{first}{separator}{second}{separator}{third}"),
                        members: vec![first.to_owned(), second.to_owned(), third.to_owned()],
                    });
                }
            }
        }
    }

    cases
}

/// Every list splits back into the tags it was written from.
#[test]
fn every_list_recovers_the_tags_it_was_written_from() {
    for case in every_list() {
        assert_eq!(
            split(&case.written).collect::<Vec<_>>(),
            case.members,
            "`{}` did not split into its members",
            case.written
        );
    }
}

/// An empty element is dropped, which RFC 9110 section 5.6.1.2 asks of a
/// recipient.
#[test]
fn a_blank_element_is_dropped_rather_than_refused() {
    assert_eq!(
        split(r#", "a" ,, "b","#).collect::<Vec<_>>(),
        [r#""a""#, r#""b""#]
    );
    assert_eq!(split("  ").count(), 0);
    assert_eq!(split("").count(), 0);
}

/// RFC 9110 section 8.8.3.2, Table 3, transcribed whole.
///
/// A closed enumeration: four pairs and two functions, so a comparison that
/// drifts in either direction fails rather than being sampled around.
#[test]
fn the_specifications_own_comparison_table_holds() {
    for (left, right, strong, weak) in [
        (r#"W/"1""#, r#"W/"1""#, false, true),
        (r#"W/"1""#, r#"W/"2""#, false, false),
        (r#"W/"1""#, r#""1""#, false, true),
        (r#""1""#, r#""1""#, true, true),
    ] {
        assert_eq!(strong_match(left, right), strong, "strong {left} {right}");
        assert_eq!(weak_match(left, right), weak, "weak {left} {right}");
        // Both functions are symmetric, which the table states only by
        // listing one order of each pair.
        assert_eq!(strong_match(right, left), strong, "strong {right} {left}");
        assert_eq!(weak_match(right, left), weak, "weak {right} {left}");
    }
}

/// The wildcard is a field value rather than a tag, so no comparison
/// answers for it.
#[test]
fn the_wildcard_is_not_an_entity_tag() {
    assert_eq!(ANY, "*");
    assert!(!weak_match(ANY, r#""1""#));
    assert!(!strong_match(ANY, r#""1""#));
}

/// A comma inside a quoted tag is part of the tag, not a separator.
///
/// RFC 9110 section 8.8.3: `etagc` is `%x21 / %x23-7E / obs-text`, which
/// includes `,` at `%x2C`. The quotes delimit an entity tag; the comma does
/// not. A field split on every comma reads `"a,b"` as two tags and matches
/// neither, which is a 200 where a 304 was owed.
#[test]
fn a_comma_inside_a_quoted_tag_does_not_separate_two_tags() {
    for (field, etag) in [
        (r#""a,b""#, r#""a,b""#),
        // The same tag among others, so the scan has to survive both.
        (r#""x", "a,b", "y""#, r#""a,b""#),
        (r#"W/"a,b""#, r#""a,b""#),
    ] {
        assert!(
            matches(&HeaderValue::from_static(field), etag),
            "`{field}` does not name `{etag}`"
        );
    }
}

/// The control: a comma *between* two quoted tags does separate them.
///
/// Differs from the case above in exactly the property under test — where the
/// comma sits — so "a comma does not split" cannot pass by refusing to split
/// at all.
#[test]
fn a_comma_between_two_quoted_tags_separates_them() {
    let field = HeaderValue::from_static(r#""a", "b""#);

    assert!(matches(&field, r#""a""#));
    assert!(matches(&field, r#""b""#));
    assert!(!matches(&field, r#""a,b""#));
    assert!(!matches(&field, r#""c""#));
}

/// `If-Match` reads the same list `If-None-Match` does, and compares each
/// member strongly.
///
/// RFC 9110 section 13.1.1: *an origin server MUST use the strong comparison
/// function when comparing entity tags for If-Match*. The cases differ from
/// one another in one property each — weakness on the field's side, weakness
/// on the representation's side, the member's position in the list, and the
/// absence of any tag to compare against.
#[test]
fn if_match_compares_every_listed_tag_strongly() {
    for (field, current, holds) in [
        (r#""r3""#, Some(r#""r3""#), true),
        (r#""r2""#, Some(r#""r3""#), false),
        (r#""r2", "r3""#, Some(r#""r3""#), true),
        (r#""x", "a,b", "y""#, Some(r#""a,b""#), true),
        (r#"W/"r3""#, Some(r#""r3""#), false),
        (r#""r3""#, Some(r#"W/"r3""#), false),
        (r#""r3""#, None, false),
    ] {
        assert_eq!(
            matches_strongly(&HeaderValue::from_static(field), current),
            holds,
            "`{field}` against {current:?}"
        );
    }
}

/// `*` holds for any current representation, tagged or not, and strongly or
/// weakly.
///
/// Section 13.1.1: *if the field value is "\*", the condition is true if the
/// origin server has a current representation for the target resource* — a
/// condition on existence, not on a validator.
#[test]
fn if_match_any_holds_whatever_the_representation_is_tagged() {
    let field = HeaderValue::from_static(" * ");

    for current in [Some(r#""r3""#), Some(r#"W/"r3""#), None] {
        assert!(matches_strongly(&field, current), "{current:?}");
    }
}

/// `If-Match` is one list across its field lines: a tag on the second line
/// holds, two lines naming neither the current tag fail, and an absent field
/// is told apart from a failed one without the tag being asked for.
#[test]
fn if_match_is_one_list_across_its_field_lines() {
    let fields = |lines: &[&'static str]| {
        let mut fields = HeaderMap::new();
        for line in lines {
            fields.append(header::IF_MATCH, HeaderValue::from_static(line));
        }
        fields
    };

    assert_eq!(
        if_match(&fields(&[r#""old""#, r#""t""#]), || Some(r#""t""#)),
        Some(true)
    );
    assert_eq!(
        if_match(&fields(&[r#""old""#, r#""older""#]), || Some(r#""t""#)),
        Some(false)
    );
    assert_eq!(
        if_match(&fields(&[]), || -> Option<&str> {
            panic!("no If-Match, so no tag to compare")
        }),
        None
    );
}
