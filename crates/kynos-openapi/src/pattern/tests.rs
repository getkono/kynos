/// [`super::translate`], with the refusal as the sentence it renders.
fn translate(pattern: &str) -> Result<String, String> {
    super::translate(pattern).map_err(|refusal| refusal.to_string())
}

/// Whether the translation of `pattern` matches `text`.
fn matches(pattern: &str, text: &str) -> bool {
    let translated = translate(pattern).unwrap_or_else(|reason| panic!("`{pattern}`: {reason}"));
    regex::Regex::new(&translated)
        .expect("a translation compiles")
        .is_match(text)
}

/// Each construct the two dialects read differently, on the characters that
/// tell the readings apart, against what ECMA-262 (with the `u` flag JSON
/// Schema asks for) says of them.
#[test]
fn a_translated_pattern_matches_as_ecma_262_reads_it() {
    for (pattern, text, ecma) in [
        // `\d` and `\D`: ASCII digits, not ARABIC-INDIC DIGIT THREE.
        (r"^\d$", "7", true),
        (r"^\d$", "\u{663}", false),
        (r"^\D$", "\u{663}", true),
        (r"^\D$", "7", false),
        (r"^[\d]$", "\u{663}", false),
        (r"^[^\d]$", "\u{663}", true),
        (r"^[\D]$", "7", false),
        // `\w` and `\W`: ASCII letters, digits and `_`, not `é`.
        (r"^\w+$", "a_Z9", true),
        (r"^\w$", "é", false),
        (r"^\W$", "é", true),
        (r"^[\w-]+$", "a-b", true),
        (r"^[\w-]+$", "é", false),
        // `\s` and `\S`: U+FEFF is ECMA-262 space and U+0085 is not, the
        // reverse of Unicode's `White_Space`.
        (r"^\s$", "\u{feff}", true),
        (r"^\s$", "\u{85}", false),
        (r"^\s$", "\u{2028}", true),
        (r"^\S$", "\u{85}", true),
        (r"^\S$", "\u{feff}", false),
        // `.`: every character but the four line terminators.
        (r"^.$", "\r", false),
        (r"^.$", "\u{2028}", false),
        (r"^.$", "\u{2029}", false),
        (r"^.$", "\n", false),
        (r"^.$", "é", true),
        (r"^.$", "\u{85}", true),
        // `\b` and `\B`: between ASCII word characters, so `é` is not one.
        (r"\bé", "é", false),
        (r"a\b", "aé", true),
        (r"a\B", "aé", false),
        (r"a\B", "ab", true),
        // Untouched: the anchors, classes, quantifiers, groups and escapes
        // both dialects read alike.
        (r"^[a-z]{2,3}$", "abc", true),
        (r"^[a-z]{2,3}$", "abcd", false),
        (r"^(?:ab|cd)+?$", "abcd", true),
        (r"^(?<year>\d{4})-\d{2}$", "2024-01", true),
        (r"^\p{Lu}\p{Script=Greek}$", "AΩ", true),
        (r"^é\u{1F600}\x41$", "é😀A", true),
        (r"^\.\/$", "./", true),
        (r"^[\-\]]+$", "-]", true),
        (r"^(?:a{2})*$", "aaaa", true),
        (r"^(?:a{2})*$", "aaa", false),
        (r"^(?<año>a)$", "a", true),
        // Not anchored unless it says so.
        ("b", "abc", true),
        ("", "anything", true),
    ] {
        assert_eq!(
            matches(pattern, text),
            ecma,
            "`{pattern}` on {text:?}: ECMA-262 says {ecma}"
        );
    }
}

/// Each refusal, by what it says. One per site in the source, so a rule
/// added without a case fails the count below.
fn refusals() -> Vec<(&'static str, &'static str)> {
    vec![
        ("(?i)abc", "an inline flag group"),
        ("(?i:abc)", "a group setting flags"),
        ("(?P<name>a)", "`(?P<name>...)`"),
        (r"\Aabc", "an assertion such as `\\A`"),
        ("[[:alpha:]]", "a POSIX class"),
        ("[a[b]]", "a class nested in a class"),
        ("[a-z&&b]", "a class set operation"),
        ("a}", "an unescaped `}`"),
        (r"\#", "`\\#`, an escape ECMA-262 refuses"),
        (r"\x{41}", "a `\\U` or `\\x{...}` escape"),
        (r"\a", "an escape such as `\\a`"),
        (r"\pL", "a one-letter Unicode class"),
        (r"\p{scx:Greek}", "a Unicode class written with `:`"),
        ("a**", "a quantifier on a quantifier"),
        (r"\b+", "a quantified assertion"),
        ("[]a]", "a `]` first in a class"),
        (
            "(?<a.b>x)",
            "a group name that is not an ECMA-262 identifier",
        ),
    ]
}

#[test]
fn a_pattern_the_dialects_read_apart_is_refused() {
    for (pattern, expects) in refusals() {
        let Err(reason) = translate(pattern) else {
            panic!("`{pattern}` must be refused");
        };
        assert!(
            reason.contains(expects) && reason.contains("ECMA-262"),
            "`{pattern}`: expected {expects:?}, got {reason:?}"
        );
    }
}

/// The other spellings of the refusals above, each of which ECMA-262 with
/// the `u` flag refuses and the engine reads.
#[test]
fn each_spelling_of_a_refused_construct_is_refused() {
    for (pattern, expects) in [
        ("x{2}{3}", "a quantifier on a quantifier"),
        ("a*??", "a quantifier on a quantifier"),
        ("^*", "a quantified assertion"),
        ("$?", "a quantified assertion"),
        (r"\B{2}", "a quantified assertion"),
        ("[^]a]", "a `]` first in a class"),
        ("[]-a]", "a `]` first in a class"),
        (
            "(?<a[0]>x)",
            "a group name that is not an ECMA-262 identifier",
        ),
        (
            "(?<a\u{bd}>x)",
            "a group name that is not an ECMA-262 identifier",
        ),
        (
            "(?<\u{345}a>x)",
            "a group name that is not an ECMA-262 identifier",
        ),
    ] {
        let Err(reason) = translate(pattern) else {
            panic!("`{pattern}` must be refused");
        };
        assert!(
            reason.contains(expects),
            "`{pattern}`: expected {expects:?}, got {reason:?}"
        );
    }
}

/// ECMA-262 admits `\-` only inside a class, where the engine agrees.
#[test]
fn an_escaped_hyphen_is_refused_outside_a_class() {
    let reason = translate(r"a\-b").expect_err(r"`\-` outside a class");
    assert!(
        reason.contains(r"`\-`, an escape ECMA-262 refuses"),
        "{reason}"
    );
    assert!(translate(r"[a\-b]").is_ok());
}

#[test]
fn every_dialect_refusal_has_a_case() {
    // The definition of `not_ecma` is the one mention that is not a site.
    let sites = include_str!("../pattern.rs").matches("not_ecma(").count() - 1;
    assert_eq!(refusals().len(), sites);
}

/// What ECMA-262 has and the engine does not is refused as that, and what is
/// no regular expression at all as that.
#[test]
fn a_pattern_the_engine_cannot_run_is_refused() {
    for pattern in ["a(?=b)", "a(?!b)", "(?<=a)b", "(?<!a)b", r"(a)\1"] {
        let reason = translate(pattern).expect_err(pattern);
        assert!(
            reason.contains("lookaround and backreferences"),
            "{pattern}: {reason}"
        );
    }

    for pattern in ["(", "a{,3}", "[b-a]"] {
        let reason = translate(pattern).expect_err(pattern);
        assert!(
            reason.contains("not a regular expression"),
            "{pattern}: {reason}"
        );
    }

    let reason = translate(r"(?:\w{1000}){1000}").expect_err("past the engine's size limit");
    assert!(reason.contains("cannot compile"), "{reason}");
}
