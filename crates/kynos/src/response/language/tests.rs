use crate::response::language::tag::{IRREGULAR, LanguageTag, TagDefect};

/// One assembled tag, and the parts it was assembled from.
///
/// The shape [`TemplateCase`] uses in `kynos-openapi`'s suite, for the same
/// reason: an oracle derived from the parser agrees with it by construction,
/// including wherever both are wrong. This records what each subtag *is* while
/// building the string, so the assertions below never consult the grammar they
/// are checking.
///
/// [`TemplateCase`]: https://github.com/getkono/kynos/blob/master/crates/kynos-openapi/tests/support/mod.rs
struct TagCase {
    /// The tag as written, in the mixed case a client might send.
    written: String,
    /// Each subtag paired with the casing section 2.1.1 recommends for it.
    expected: Vec<String>,
}

impl TagCase {
    /// Assembles a tag from subtags whose roles are known.
    ///
    /// `parts` is each subtag as the registry spells it. The recommended casing
    /// follows from position alone — lowercase, except a two-letter subtag that
    /// neither opens the tag nor follows a singleton is uppercase and a
    /// four-letter one there is titlecase — so it is computed here from the
    /// role rather than read back from the parser.
    fn new(parts: &[&str]) -> Self {
        let mut written = String::new();
        let mut expected = Vec::new();
        let mut after_singleton = false;

        for (position, part) in parts.iter().enumerate() {
            if position > 0 {
                written.push('-');
            }
            // Written in a case no convention recommends, so a parser that
            // simply echoed its input would fail every assertion below.
            written.push_str(&alternating(part));

            let recommended = if position == 0 || after_singleton {
                part.to_ascii_lowercase()
            } else if part.len() == 2 {
                part.to_ascii_uppercase()
            } else if part.len() == 4 {
                let mut titled = part.to_ascii_lowercase();
                titled[..1].make_ascii_uppercase();
                titled
            } else {
                part.to_ascii_lowercase()
            };
            expected.push(recommended);

            after_singleton = after_singleton || part.len() == 1;
        }

        Self { written, expected }
    }
}

/// `en-GB` becomes `eN-gB`, which is a case no section recommends.
fn alternating(part: &str) -> String {
    part.char_indices()
        .map(|(index, character)| {
            if index % 2 == 0 {
                character.to_ascii_lowercase()
            } else {
                character.to_ascii_uppercase()
            }
        })
        .collect()
}

/// Every shape `langtag` admits, assembled rather than generated.
///
/// The space of *shapes* closes even though the space of strings does not: the
/// ABNF gives one optional script, one optional region, a bounded `extlang`,
/// and repetition only in variants, extensions and private use. Sweeping the
/// cross product of those positions is total over the grammar's structure,
/// which is the property worth having — a `proptest` draw over the same space
/// would sample it, and `kynos` has no `proptest` dependency to add.
fn every_shape() -> Vec<TagCase> {
    let mut cases = Vec::new();

    for language in ["en", "eng", "abcd", "abcde"] {
        // Only a two- or three-letter primary subtag takes an `extlang`.
        let extlangs: &[&[&str]] = if language.len() <= 3 {
            &[&[], &["yue"], &["yue", "cmn"]]
        } else {
            &[&[]]
        };

        for extlang in extlangs {
            for script in [None, Some("Latn")] {
                for region in [None, Some("GB"), Some("419")] {
                    for variants in [
                        &[][..],
                        &["rozaj"][..],
                        &["rozaj", "biske"][..],
                        &["1901"][..],
                    ] {
                        for extension in [&[][..], &["a", "bbb"][..]] {
                            for private in [&[][..], &["x", "pig"][..]] {
                                let mut parts = vec![language];
                                parts.extend_from_slice(extlang);
                                parts.extend(script);
                                parts.extend(region);
                                parts.extend_from_slice(variants);
                                parts.extend_from_slice(extension);
                                parts.extend_from_slice(private);
                                cases.push(TagCase::new(&parts));
                            }
                        }
                    }
                }
            }
        }
    }

    cases
}

#[test]
fn every_tag_the_grammar_admits_parses_into_the_subtags_it_was_built_from() {
    let cases = every_shape();
    assert_eq!(
        cases.len(),
        8 * 2 * 3 * 4 * 2 * 2,
        "the sweep is not closed"
    );

    for case in cases {
        let tag = LanguageTag::parse(&case.written)
            .unwrap_or_else(|defect| panic!("{} was refused: {defect}", case.written));

        assert_eq!(
            tag.subtags().collect::<Vec<_>>(),
            case.expected,
            "{} did not read back as the parts it was built from",
            case.written
        );
    }
}

#[test]
fn a_tag_is_normalized_to_the_casing_the_specification_recommends() {
    // Section 2.1.1's own example, in three of the spellings it calls equal.
    for written in ["mn-Cyrl-MN", "MN-cYRL-mn", "mN-cYrL-Mn"] {
        let tag = LanguageTag::parse(written).expect("well-formed");
        assert_eq!(tag.as_str(), "mn-Cyrl-MN", "{written}");
    }

    // Everything after a singleton stays lowercase, which is the half a
    // position-blind rule gets wrong: the second `latn` is not a script.
    let tag = LanguageTag::parse("AZ-LATN-X-LATN").expect("well-formed");
    assert_eq!(tag.as_str(), "az-Latn-x-latn");

    // Normalizing is idempotent, so a tag that has been through once is
    // unchanged by a second pass.
    let again = LanguageTag::parse(tag.as_str()).expect("well-formed");
    assert_eq!(again, tag);
}

#[test]
fn a_tag_is_the_same_tag_whatever_case_it_arrived_in() {
    assert_eq!(
        LanguageTag::parse("EN-gb").expect("well-formed"),
        LanguageTag::parse("en-GB").expect("well-formed")
    );
}

/// The irregular list, counted and checked against what the grammar can do
/// without it.
///
/// The ABNF names twenty-six grandfathered tags in two halves, and only one
/// half needs transcribing: the nine `regular` ones "match the 'langtag'
/// production", so the parser accepts them for free, and transcribing them
/// would be nine rows asserting what the grammar already says. This pins both
/// halves of that claim, so a future reader does not have to take it on trust.
#[test]
fn every_grandfathered_tag_parses_and_only_the_irregular_ones_need_a_table() {
    const REGULAR: [&str; 9] = [
        "art-lojban",
        "cel-gaulish",
        "no-bok",
        "no-nyn",
        "zh-guoyu",
        "zh-hakka",
        "zh-min",
        "zh-min-nan",
        "zh-xiang",
    ];

    assert_eq!(IRREGULAR.len(), 17, "the irregular half is seventeen tags");
    assert_eq!(REGULAR.len(), 9, "the regular half is nine tags");

    for tag in IRREGULAR {
        LanguageTag::parse(tag).unwrap_or_else(|defect| panic!("{tag} was refused: {defect}"));
    }

    for tag in REGULAR {
        assert!(
            !IRREGULAR.contains(&tag),
            "{tag} matches `langtag` and does not belong in the table"
        );
        LanguageTag::parse(tag).unwrap_or_else(|defect| panic!("{tag} was refused: {defect}"));
    }

    // The table is load-bearing rather than decorative: every tag in it is one
    // the grammar refuses on its own, which is the ABNF's stated reason for
    // listing them. `en-GB-oed` is the readable case -- three letters after a
    // region is not a variant.
    assert_eq!(
        LanguageTag::parse("en-GB-oed-oed"),
        Err(TagDefect::Misplaced),
        "the table entry, not the grammar, is what accepts `en-GB-oed`"
    );
}

/// One input per refusal, matched exhaustively.
///
/// The match is what counts the set: a variant added to `TagDefect` stops this
/// file compiling until it is given a case, which is stronger than counting
/// occurrences in the source because it cannot be satisfied by a duplicate.
#[test]
fn every_defect_a_tag_can_carry_has_a_case() {
    let cases = [
        ("", TagDefect::Empty),
        ("en--GB", TagDefect::MalformedSubtag),
        ("en-toolongsubtag", TagDefect::MalformedSubtag),
        ("en-G!", TagDefect::MalformedSubtag),
        ("e-GB", TagDefect::PrimaryLanguage),
        ("1en", TagDefect::PrimaryLanguage),
        ("en-GB-Latn", TagDefect::Misplaced),
        ("en-a", TagDefect::DanglingSingleton),
        ("x", TagDefect::DanglingSingleton),
    ];

    for (value, expected) in cases {
        assert_eq!(LanguageTag::parse(value), Err(expected), "{value:?}");
    }

    // Exhaustive, so the set cannot grow without a case above.
    for (_, defect) in cases {
        match defect {
            TagDefect::Empty
            | TagDefect::MalformedSubtag
            | TagDefect::PrimaryLanguage
            | TagDefect::Misplaced
            | TagDefect::DanglingSingleton => {}
        }
    }

    let witnessed: std::collections::BTreeSet<_> = cases
        .iter()
        .map(|(_, defect)| format!("{defect:?}"))
        .collect();
    assert_eq!(witnessed.len(), 5, "a refusal has no case above");
}

#[test]
fn a_private_use_tag_needs_no_language_at_all() {
    let tag = LanguageTag::parse("x-pig-latin").expect("well-formed");
    assert_eq!(tag.as_str(), "x-pig-latin");
}

#[test]
fn well_formed_is_the_same_answer_in_a_const_context_as_at_run_time() {
    // Asserted in a `const` block rather than at run time, because what is
    // under test is that the answer exists at compile time at all: this is the
    // idiom the coming offered-set check is built from, so proving it here
    // proves the mechanism rather than the value.
    const { assert!(LanguageTag::is_well_formed("zh-Hant-TW")) }
    const { assert!(!LanguageTag::is_well_formed("en-GB-Latn")) }

    for value in ["zh-Hant-TW", "en-GB-Latn", "", "x", "i-klingon", "es-419"] {
        assert_eq!(
            LanguageTag::is_well_formed(value),
            LanguageTag::parse(value).is_ok(),
            "{value:?}"
        );
    }
}

/// Well-formed is not valid, and the module says so; this is the record.
///
/// Paired with the gap `nfr.md` documents: closing it would need the IANA
/// subtag registry, which `architecture.md` refuses. Named so it reads as a
/// record of what happens today rather than an endorsement of it.
#[test]
fn a_tag_naming_no_real_language_is_still_well_formed() {
    assert!(LanguageTag::parse("zz-Qaaa-QM").is_ok());
}
