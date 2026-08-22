//! Entity tags: reading a list of them, and the two ways to compare two.
//!
//! # The grammar
//!
//! RFC 9110 section 8.8.3:
//!
//! ```text
//! entity-tag = [ weak ] opaque-tag
//! weak       = %s"W/"
//! opaque-tag = DQUOTE *etagc DQUOTE
//! etagc      = %x21 / %x23-7E / obs-text
//!            ; VCHAR except double quotes, plus obs-text
//! ```
//!
//! Transcribed rather than cited, because one character decides the whole of
//! [`split`]: `etagc` admits `,` at `%x2C`. A comma *inside* the quotes is part
//! of the tag and only a comma outside them separates two, so the quotes are
//! the delimiter and the comma is not. A reader that splits on every comma
//! takes `"a,b"` for two tags and matches neither — a 200 where a 304 was owed,
//! and the kind of thing a comparator is either right about or silently wrong
//! about forever.
//!
//! # Why the comparison lives here
//!
//! Three call sites compare entity tags and no two of them share a feature:
//! [`middleware::conditional`](crate::middleware::conditional) is behind
//! `cache`, [`router::assets`](crate::router::assets) behind `assets`, and
//! `If-Range` — in [`response::range`](crate::response::range) — is behind
//! neither and wants the other comparison function besides. Nothing gated can
//! be the single implementation, so it sits with the field it reads instead,
//! beside [`cookie`](super::cookie): the other field whose grammar Kynos reads
//! rather than looks up.
//!
//! Public for the same reason `cookie` is. A handler that mints its own
//! validator and evaluates its own precondition needs exactly these four
//! functions, and the alternative to exporting them is every application
//! writing the comma scan again.

/// `*`, which matches any current representation the server has.
pub const ANY: &str = "*";

/// Splits a `1#entity-tag` field value into its members, trimmed.
///
/// Quote-aware, per the grammar above. An empty element is dropped rather than
/// refused: RFC 9110 section 5.6.1.2 asks a recipient of a `#` list to accept
/// them.
#[must_use]
pub fn split(text: &str) -> Vec<&str> {
    let mut tags = Vec::new();
    let mut quoted = false;
    let mut start = 0;

    for (index, character) in text.char_indices() {
        match character {
            // `etagc` excludes DQUOTE and `opaque-tag` has no escape, so every
            // quote is a delimiter and toggling on it is the whole of the scan.
            '"' => quoted = !quoted,
            ',' if !quoted => {
                push(&mut tags, &text[start..index]);
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    push(&mut tags, &text[start..]);

    tags
}

/// Records `candidate` as a tag unless it is blank.
fn push<'a>(tags: &mut Vec<&'a str>, candidate: &'a str) {
    let trimmed = candidate.trim();
    if !trimmed.is_empty() {
        tags.push(trimmed);
    }
}

/// Whether `tag` carries the weakness marker.
#[must_use]
pub fn is_weak(tag: &str) -> bool {
    tag.starts_with("W/")
}

/// A tag's `opaque-tag`, which is itself with any weakness marker removed.
#[must_use]
pub fn opaque(tag: &str) -> &str {
    tag.strip_prefix("W/").unwrap_or(tag)
}

/// RFC 9110 section 8.8.3.2's *weak comparison*.
///
/// *Two entity tags are equivalent if their opaque-tags match
/// character-by-character, regardless of either or both being tagged as
/// "weak".* This is the one `If-None-Match` takes: a cache validation asks
/// whether the stored copy is still good enough, not whether it is identical.
#[must_use]
pub fn weak_match(left: &str, right: &str) -> bool {
    opaque(left) == opaque(right)
}

/// RFC 9110 section 8.8.3.2's *strong comparison*.
///
/// *Two entity tags are equivalent if both are not weak and their opaque-tags
/// match character-by-character.* This is the one `If-Range` takes, per section
/// 13.1.5 — a part is only safe to splice into a copy the client already holds
/// if the representation is byte-for-byte the one that copy came from. A weak
/// tag therefore satisfies nothing here, on either side.
#[must_use]
pub fn strong_match(left: &str, right: &str) -> bool {
    !is_weak(left) && !is_weak(right) && left == right
}

#[cfg(test)]
mod tests {
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
                split(&case.written),
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
        assert_eq!(split(r#", "a" ,, "b","#), [r#""a""#, r#""b""#]);
        assert!(split("  ").is_empty());
        assert!(split("").is_empty());
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
}
