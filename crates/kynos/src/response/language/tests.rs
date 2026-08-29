use crate::response::language::tag::{LanguageTag, TagDefect, grammar::IRREGULAR};

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

// -- RFC 4647 matching --------------------------------------------------------

use crate::response::language::matching::{MatchKind, classify};

/// RFC 4647 section 3.3.1, transcribed.
///
/// "A language range matches a particular language tag if, in a
/// case-insensitive comparison, it exactly equals the tag, or if it exactly
/// equals a prefix of the tag such that the first character following the
/// prefix is `-`." Written as string operations, which is a different
/// computation from the subtag walk under test rather than a copy of it.
fn basic_filtering(range: &str, tag: &str) -> bool {
    if range == "*" {
        return true;
    }
    let range = range.to_ascii_lowercase();
    let tag = tag.to_ascii_lowercase();
    tag == range || tag.starts_with(&format!("{range}-"))
}

/// RFC 4647 section 3.4, transcribed as the truncation loop the text describes.
///
/// "The language range is progressively truncated from the end until a matching
/// language tag is located. Single letter or digit subtags ... are removed at
/// the same time as their closest trailing subtag."
fn lookup(range: &str, tag: &str) -> bool {
    if range == "*" {
        return false;
    }
    let mut range = range.to_ascii_lowercase();
    let tag = tag.to_ascii_lowercase();

    loop {
        if range == tag {
            return true;
        }
        let Some((head, _)) = range.rsplit_once('-') else {
            return false;
        };
        range = head.to_owned();
        // A truncation that leaves a trailing singleton removes it too.
        if range.rsplit('-').next().is_some_and(|last| last.len() == 1) {
            let Some((head, _)) = range.rsplit_once('-') else {
                return false;
            };
            range = head.to_owned();
        }
    }
}

/// Every range-and-tag pair over a closed alphabet, against both transcriptions.
///
/// The alphabet is one subtag of each length that behaves differently -- a
/// singleton, a two-letter subtag, a three-letter one -- and sequences of one
/// to three of them. That is 39 tags and 40 ranges, so 1,560 pairs, swept
/// rather than sampled: the space closes, and `docs/testing.md` says a sweep is
/// the stronger statement where it does.
#[test]
fn every_pair_of_a_range_and_a_tag_classifies_as_the_two_schemes_say() {
    let alphabet = ["x", "yy", "zzz"];
    let mut sequences = Vec::new();
    for first in alphabet {
        sequences.push(first.to_owned());
        for second in alphabet {
            sequences.push(format!("{first}-{second}"));
            for third in alphabet {
                sequences.push(format!("{first}-{second}-{third}"));
            }
        }
    }
    assert_eq!(sequences.len(), 3 + 9 + 27, "the alphabet is not closed");

    let mut ranges = sequences.clone();
    ranges.push("*".to_owned());

    let mut swept = 0;
    for range in &ranges {
        for tag in &sequences {
            let observed = classify(range, tag);
            let filters = basic_filtering(range, tag);
            let looks_up = lookup(range, tag);

            assert_eq!(
                observed.is_some(),
                filters || looks_up,
                "{range} against {tag}"
            );

            match observed.map(|(kind, _)| kind) {
                None => {}
                Some(MatchKind::Exact) => assert!(range.eq_ignore_ascii_case(tag)),
                Some(MatchKind::Wildcard) => assert_eq!(range, "*"),
                Some(MatchKind::Truncates) => {
                    assert!(looks_up, "{range} against {tag}");
                    assert!(!range.eq_ignore_ascii_case(tag));
                }
                Some(MatchKind::Extends) => {
                    assert!(filters, "{range} against {tag}");
                    assert!(!looks_up, "{range} against {tag}");
                    assert_ne!(range, "*");
                }
            }

            swept += 1;
        }
    }

    assert_eq!(swept, 40 * 39, "the sweep is not closed");
}

#[test]
fn a_range_and_a_tag_that_diverge_match_under_neither_scheme() {
    assert_eq!(classify("en-GB", "en-US"), None);
    assert_eq!(classify("en-US", "en-GB"), None);
}

#[test]
fn a_truncation_never_stops_on_a_trailing_singleton() {
    // RFC 4647 section 3.4's own worked example: the range falls from
    // `zh-Hant-CN-x-private1` straight to `zh-Hant-CN`.
    assert_eq!(
        classify("zh-Hant-CN-x-private1", "zh-Hant-CN"),
        Some((MatchKind::Truncates, 3))
    );
    assert_eq!(classify("zh-Hant-CN-x-private1", "zh-Hant-CN-x"), None);
}

#[test]
fn matching_ignores_case_in_both_directions() {
    assert_eq!(classify("EN-gb", "en-GB"), Some((MatchKind::Exact, 2)));
    assert_eq!(classify("en", "EN-GB"), Some((MatchKind::Extends, 1)));
}

#[test]
fn a_lookup_match_is_ranked_above_a_filtering_one() {
    assert!(MatchKind::Truncates > MatchKind::Extends);
    assert!(MatchKind::Exact > MatchKind::Truncates);
    assert!(MatchKind::Extends > MatchKind::Wildcard);
}

// -- ranking one offer against a priority list --------------------------------

use crate::response::language::matching::{Preference, RangeDefect, select};

/// Reads a field the way the extractor will, dropping what it cannot read.
fn preferences(field: &str) -> Vec<Preference> {
    field
        .split(',')
        .enumerate()
        .filter_map(|(order, entry)| Preference::parse(entry, order).ok())
        .collect()
}

fn chosen<'a>(field: &str, tags: &[&'a str]) -> Option<&'a str> {
    select(&preferences(field), tags).map(|index| tags[index])
}

#[test]
fn a_lookup_match_outranks_a_filtering_one_for_the_same_client() {
    // `en-GB` diverges from `en-US` and matches under neither scheme, so the
    // only candidate is `en` -- reached by truncation, which is the half plain
    // Basic Filtering would have abandoned.
    assert_eq!(chosen("en-US", &["en-GB", "en"]), Some("en"));
}

#[test]
fn the_deepest_truncation_wins_among_lookup_matches() {
    // This is section 3.4's progressive truncation, stated as a ranking: the
    // range stops at the first tag it reaches, which is the deepest one.
    assert_eq!(chosen("zh-Hant-TW", &["zh", "zh-Hant"]), Some("zh-Hant"));
}

#[test]
fn the_first_offered_tag_breaks_a_tie() {
    // Both extend `en` by one subtag at the same weight, so nothing the client
    // said separates them and the service's own order decides.
    assert_eq!(chosen("en", &["en-GB", "en-US"]), Some("en-GB"));
}

#[test]
fn an_exact_match_outranks_a_tag_that_merely_extends_the_range() {
    assert_eq!(chosen("en", &["en-GB", "en"]), Some("en"));
}

#[test]
fn a_more_specific_range_sets_the_quality_a_tag_is_scored_at() {
    // `en-GB` is named outright at 0.1, so it is scored there rather than at
    // the 0.9 the broader `en` would have given it -- and `en-AU`, which only
    // `en` names, keeps 0.9 and wins. The rule `Accept` already applies to
    // media ranges.
    assert_eq!(
        chosen("en;q=0.9, en-GB;q=0.1", &["en-GB", "en-AU"]),
        Some("en-AU")
    );
}

#[test]
fn a_range_refused_outright_never_selects_the_tag_it_names() {
    // `q=0` is "not acceptable", and it is the most specific range naming
    // `en-GB`, so `en-GB` is out even though the broader `en` is welcome.
    assert_eq!(
        chosen("en;q=1, en-GB;q=0", &["en-GB", "en-AU"]),
        Some("en-AU")
    );
}

#[test]
fn the_wildcard_scores_only_the_tags_no_other_range_named() {
    // RFC 9110 section 12.4.3: a wildcard "selects unspecified values". `fr` is
    // specified, at 0.1, so the 0.9 wildcard reaches `en` alone -- and `en`
    // therefore wins despite the client naming `fr` and not `en`.
    assert_eq!(chosen("fr;q=0.1, *;q=0.9", &["en", "fr"]), Some("en"));
}

#[test]
fn a_client_whose_preferences_match_nothing_selects_nothing() {
    // The caller turns this into the default rather than a 406; see
    // `AcceptLanguage`.
    assert_eq!(chosen("ja", &["en", "fr"]), None);
}

#[test]
fn a_field_of_nothing_readable_selects_nothing_rather_than_refusing() {
    assert_eq!(chosen("!!!, ;;;", &["en"]), None);
}

/// One input per refusal, matched exhaustively.
#[test]
fn every_defect_a_range_can_carry_has_a_case() {
    let cases = [
        ("", RangeDefect::Empty),
        ("1en", RangeDefect::PrimarySubtag),
        ("toolongprimary", RangeDefect::PrimarySubtag),
        ("en-toolongsubtag", RangeDefect::Subtag),
        ("en-", RangeDefect::Subtag),
        ("en;q=1.5", RangeDefect::Weight),
        ("en;nonsense", RangeDefect::Weight),
    ];

    for (entry, expected) in cases {
        assert_eq!(Preference::parse(entry, 0), Err(expected), "{entry:?}");
    }

    for (_, defect) in cases {
        match defect {
            RangeDefect::Empty
            | RangeDefect::PrimarySubtag
            | RangeDefect::Subtag
            | RangeDefect::Weight => {}
        }
    }

    let witnessed: std::collections::BTreeSet<_> = cases
        .iter()
        .map(|(_, defect)| format!("{defect:?}"))
        .collect();
    assert_eq!(witnessed.len(), 4, "a refusal has no case above");
}

#[test]
fn an_unreadable_range_is_dropped_and_the_rest_of_the_field_still_counts() {
    // A field the server can partly read is one it should partly honour. The
    // same call `http::date` makes, and for the same reason: refusing the whole
    // request would decline to serve something the specification says to serve.
    assert_eq!(chosen("!!!, fr", &["en", "fr"]), Some("fr"));
}

#[test]
fn a_range_carrying_no_weight_is_read_as_the_strongest_preference() {
    // Section 12.4.2: "If no 'q' parameter is present, the default weight is 1."
    assert_eq!(chosen("fr, en;q=0.9", &["en", "fr"]), Some("fr"));
}

// -- the extractor ------------------------------------------------------------

use crate::response::language::{AcceptLanguage, offer::Languages};

struct Supported;

impl Languages for Supported {
    const TAGS: &'static [&'static str] = &["en", "fr", "de"];
}

#[test]
fn a_client_whose_preferences_match_nothing_is_served_the_first_tag_offered() {
    // The 406 that is deliberately not raised. RFC 9110 section 15.5.7 defines
    // that status as the server being *unwilling* to supply a default; Kynos is
    // willing, and says which language it chose on the way out.
    assert_eq!(AcceptLanguage::<Supported>::parse("ja").choose(), "en");
}

#[test]
fn a_request_carrying_no_field_is_served_the_first_tag_offered() {
    assert_eq!(AcceptLanguage::<Supported>::parse("").choose(), "en");
}

#[test]
fn a_field_of_nothing_readable_is_served_the_default_rather_than_refused() {
    // The 400 that is deliberately not raised.
    assert_eq!(
        AcceptLanguage::<Supported>::parse("!!!, ;;;").choose(),
        "en"
    );
}

#[test]
fn every_tag_a_client_can_be_served_is_one_the_offer_declares() {
    // What makes the emitted `Content-Language` enumeration true. Asserted over
    // the whole space a client can ask for rather than one field, because the
    // claim is about `choose` and not about any particular request: no input
    // reaches a tag the description does not list.
    for field in [
        "fr-CA",
        "ja",
        "",
        "!!!",
        "*",
        "en;q=0",
        "de-AT, fr;q=0.4",
        "zh-Hant-TW",
        "en, fr, de",
        "*;q=0.1, ja;q=0.9",
    ] {
        let chosen = AcceptLanguage::<Supported>::parse(field).choose();
        assert!(
            Supported::TAGS.contains(&chosen),
            "{field:?} was served {chosen:?}, which the offer does not declare"
        );
    }

    assert_eq!(AcceptLanguage::<Supported>::parse("fr-CA").choose(), "fr");
}

#[test]
fn a_truncated_range_reaches_the_broader_tag_the_service_offers() {
    // The row plain Basic Filtering loses: the client asked for Canadian
    // French and the service offers only `fr`.
    assert_eq!(
        AcceptLanguage::<Supported>::parse("fr-CA, en;q=0.5").choose(),
        "fr"
    );
}

#[test]
fn a_weight_orders_the_offer_rather_than_the_field() {
    assert_eq!(
        AcceptLanguage::<Supported>::parse("de;q=0.2, fr;q=0.8").choose(),
        "fr"
    );
}

/// A priority list is a *priority* list, and its order is what says so.
///
/// RFC 4647 section 3.4: "each language range in the language priority list is
/// considered in turn, according to priority". RFC 9110 section 12.5.4 records
/// that many user agents list in decreasing order, and section 12.4.2 makes an
/// absent `q` equal to 1 -- so a client writing `de, en` has stated a
/// preference that weights alone cannot see.
#[test]
fn the_order_a_client_lists_its_ranges_in_is_a_preference() {
    // The plain case: nothing is weighted, so only the order says anything.
    assert_eq!(chosen("de, en", &["en", "de"]), Some("de"));
    assert_eq!(chosen("en, de", &["en", "de"]), Some("en"));
    assert_eq!(chosen("en, de", &["de", "en"]), Some("en"));

    // And it holds across the matching schemes: a truncated first choice beats
    // an exact second one, which is section 3.4's "first matching tag found,
    // according to the user's priority".
    assert_eq!(chosen("fr-CA, en", &["en", "fr"]), Some("fr"));
    assert_eq!(chosen("en, fr-CA", &["en", "fr"]), Some("en"));
}

/// A weight still outranks the order, because section 12.4.2 is normative
/// about what a weight means and the order is a convention on top of it.
#[test]
fn a_weight_outranks_the_order_it_was_written_in() {
    assert_eq!(chosen("de;q=0.2, en;q=0.8", &["en", "de"]), Some("en"));
}

/// The more specific of two ranges sets the weight, measured by the range.
///
/// Both of these truncate to `en`, so the depth they share says nothing about
/// which is more specific -- that is a property of the range, and reading it
/// off the shared prefix made the longer range invisible.
#[test]
fn the_longer_of_two_truncating_ranges_is_the_more_specific_one() {
    // `en` is scored through `en-US-x-y` at 0.9, so it beats an `fr` named at
    // 0.5 -- where scoring it through the earlier, shorter `en-GB` would not.
    assert_eq!(
        chosen("en-GB;q=0.2, en-US-x-y;q=0.9, fr;q=0.5", &["fr", "en"]),
        Some("en")
    );
}
