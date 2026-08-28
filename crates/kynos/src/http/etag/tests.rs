use super::{ANY, split, strong_match, weak_match};

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
