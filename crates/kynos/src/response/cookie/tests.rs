use std::time::Duration;

use super::{Cookie, SameSite};

/// What a cookie renders to, or `None`.
fn rendered(cookie: &Cookie) -> Option<String> {
    cookie
        .encode()
        .map(|value| value.to_str().expect("a printable field").to_owned())
}

/// The attribute set is closed, and the sweep below covers all of it.
///
/// `every_attribute_reaches_the_field_in_order` asserts the whole rendered
/// string, which is the right shape for order -- but it names its attributes,
/// and a name is not a count. An eighth builder would render an eighth
/// attribute and turn nothing red, because the sweep would simply not call it.
///
/// The declared side is read off the source rather than transcribed, the way
/// `tests/interceptors.rs` walks `src/middleware/` rather than listing it: a
/// transcribed list is a third place the set is written down, and it drifts.
/// Every attribute builder takes `mut self` and returns `Self`, which is what
/// separates them from `new`, `removal`, `name` and `encode`.
///
/// The scan reads the shape rather than a line. Comment lines go first, so a
/// `pub fn` inside a doc example is not mistaken for a declaration, and what
/// remains is collapsed onto one line, so a signature rustfmt has wrapped over
/// four lines reads the same as one that fits on one. `const` is tolerated
/// between `pub` and `fn`, and both halves of the shape are required: a
/// `mut self` receiver *and* a `-> Self` return, so a future method that
/// consumes a cookie without building one is not reported as an unexercised
/// attribute.
#[test]
fn every_attribute_builder_is_covered_by_the_sweep() {
    let source = include_str!("../cookie.rs");
    let code = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ");

    let declared: std::collections::BTreeSet<&str> = code
        .split("pub ")
        .skip(1)
        .map(|item| item.strip_prefix("const ").unwrap_or(item))
        .filter_map(|item| item.strip_prefix("fn ")?.split_once('('))
        .filter_map(|(name, rest)| {
            // Up to the body, or to the `;` of a signature that has none.
            let signature = rest.split(['{', ';']).next()?.trim();
            // The receiver, and nothing that merely starts like it: collapsing
            // a wrapped signature leaves `mut self ,` rather than `mut self,`.
            let after_receiver = signature.strip_prefix("mut self")?.trim_start();
            let takes_mut_self = after_receiver.starts_with(',') || after_receiver.starts_with(')');
            (takes_mut_self && signature.contains("-> Self")).then_some(name.trim())
        })
        .collect();

    let swept = std::collections::BTreeSet::from([
        "domain",
        "http_only",
        "max_age",
        "partitioned",
        "path",
        "same_site",
        "secure",
    ]);

    assert_eq!(
        declared, swept,
        "an attribute builder is not exercised by `every_attribute_reaches_the_field_in_order`; add it to that cookie and to this set, or the field it renders is asserted nowhere"
    );
}

/// Every attribute, in the order RFC 6265bis lists them.
///
/// A sweep of the shape rather than one case per attribute: the order is part
/// of what a reader checks by eye, and asserting the whole string is what
/// catches an attribute inserted in the wrong place.
#[test]
fn every_attribute_reaches_the_field_in_order() {
    let cookie = Cookie::new("session", "abc123")
        .path("/app")
        .domain("example.com")
        .max_age(Duration::from_secs(3_600))
        .secure()
        .http_only()
        .same_site(SameSite::Strict)
        .partitioned();

    assert_eq!(
        rendered(&cookie).as_deref(),
        Some(
            "session=abc123; Path=/app; Domain=example.com; Max-Age=3600; Secure; HttpOnly; \
             SameSite=Strict; Partitioned"
        )
    );
}

#[test]
fn a_bare_cookie_carries_no_attribute_it_was_not_given() {
    assert_eq!(rendered(&Cookie::new("a", "1")).as_deref(), Some("a=1"));
}

/// A removal is `Max-Age=0`, and carries the scope the browser keys on.
#[test]
fn a_removal_expires_the_cookie_it_names() {
    assert_eq!(
        rendered(&Cookie::removal("session").path("/app")).as_deref(),
        Some("session=; Path=/app; Max-Age=0")
    );
}

/// A lifetime is rounded up to the next whole second, so only an explicit
/// zero is ever sent as the removal `Max-Age=0`, and a cookie never lives
/// shorter than it was asked to.
#[test]
fn a_fractional_lifetime_rounds_up_rather_than_removing_the_cookie() {
    let max_age = |age| rendered(&Cookie::new("a", "1").max_age(age));
    assert_eq!(
        max_age(Duration::from_nanos(1)).as_deref(),
        Some("a=1; Max-Age=1")
    );
    assert_eq!(
        max_age(Duration::from_millis(500)).as_deref(),
        Some("a=1; Max-Age=1")
    );
    assert_eq!(
        max_age(Duration::from_millis(1_500)).as_deref(),
        Some("a=1; Max-Age=2")
    );
    assert_eq!(
        max_age(Duration::from_secs(2)).as_deref(),
        Some("a=1; Max-Age=2")
    );
    assert_eq!(max_age(Duration::ZERO).as_deref(), Some("a=1; Max-Age=0"));
    assert_eq!(
        max_age(Duration::MAX).as_deref(),
        Some(format!("a=1; Max-Age={}", u64::MAX).as_str())
    );
}

/// `SameSite=None` implies `Secure`, and is sent with it whether or not it was
/// asked for.
///
/// Every current browser rejects the pair's absence *silently*, which is the
/// worst way to learn — the service believes it set a cookie the client never
/// stored.
#[test]
fn same_site_none_carries_secure_without_being_asked() {
    let rendered =
        rendered(&Cookie::new("a", "1").same_site(SameSite::None)).expect("a representable cookie");

    assert!(rendered.contains("; Secure"), "{rendered}");
    assert!(rendered.contains("; SameSite=None"), "{rendered}");
}

/// `Partitioned` implies `Secure` too: CHIPS (draft-cutler-httpbis-partitioned-
/// cookies section 2.1) has a user agent ignore a partitioned cookie set
/// without it, as silently as the pair above.
#[test]
fn a_partitioned_cookie_carries_secure_without_being_asked() {
    let rendered = rendered(&Cookie::new("a", "1").partitioned()).expect("a legal cookie");

    assert!(rendered.contains("; Secure"), "{rendered}");
    assert!(rendered.contains("; Partitioned"), "{rendered}");
}

/// The other two values imply nothing.
#[test]
fn the_other_same_site_values_imply_no_attribute() {
    for value in [SameSite::Strict, SameSite::Lax] {
        let rendered =
            rendered(&Cookie::new("a", "1").same_site(value)).expect("a representable cookie");
        assert!(!rendered.contains("Secure"), "{value:?}: {rendered}");
    }
}

/// Each place `encode` refuses, by what it checks.
const REFUSALS: [&str; 6] = [
    "name or value outside its grammar",
    "name and value over 4096 octets",
    "__Host- with a Domain",
    "__Host- with a Path other than /",
    "path outside the attribute grammar",
    "domain outside the domain grammar",
];

/// One case per way `encode` refuses, and per clause of each grammar it
/// refuses by: the refusal it reaches, what it is, and the cookie.
#[expect(clippy::too_many_lines)]
fn refused() -> Vec<(&'static str, &'static str, Cookie)> {
    let long_label = "a".repeat(64);
    vec![
        (
            REFUSALS[0],
            "a name carrying a space",
            Cookie::new("a b", "1"),
        ),
        (REFUSALS[0], "an empty name", Cookie::new("", "1")),
        (
            REFUSALS[0],
            "a name carrying a control",
            Cookie::new("a\u{1}", "1"),
        ),
        (
            REFUSALS[0],
            "a name that is not ASCII",
            Cookie::new("caf\u{e9}", "1"),
        ),
        (
            REFUSALS[0],
            "a value carrying a semicolon",
            Cookie::new("a", "1; Path=/"),
        ),
        (
            REFUSALS[0],
            "a value carrying a comma",
            Cookie::new("a", "1,2"),
        ),
        (
            REFUSALS[0],
            "a value carrying a quote",
            Cookie::new("a", "\"1\""),
        ),
        (
            REFUSALS[0],
            "a value carrying a backslash",
            Cookie::new("a", "1\\2"),
        ),
        (
            REFUSALS[0],
            "a value carrying a space",
            Cookie::new("a", "1 2"),
        ),
        (
            REFUSALS[0],
            "a value carrying a control",
            Cookie::new("a", "1\u{1}"),
        ),
        (
            REFUSALS[0],
            "a value carrying DEL",
            Cookie::new("a", "1\u{7f}"),
        ),
        (
            REFUSALS[0],
            "a value that is not ASCII",
            Cookie::new("a", "caf\u{e9}"),
        ),
        (
            REFUSALS[1],
            "a name and value over 4096 octets",
            Cookie::new("a", "x".repeat(4096)),
        ),
        (
            REFUSALS[2],
            "a __Host- cookie with a Domain",
            Cookie::new("__Host-a", "1").domain("example.com"),
        ),
        (
            REFUSALS[3],
            "a __Host- cookie scoped to a path other than /",
            Cookie::new("__Host-a", "1").path("/admin"),
        ),
        (
            REFUSALS[4],
            "a path carrying a semicolon",
            Cookie::new("a", "1").path("/x;y"),
        ),
        (REFUSALS[4], "an empty path", Cookie::new("a", "1").path("")),
        (
            REFUSALS[4],
            "a path carrying a control",
            Cookie::new("a", "1").path("/x\u{1}"),
        ),
        (
            REFUSALS[4],
            "a path that is not ASCII",
            Cookie::new("a", "1").path("/caf\u{e9}"),
        ),
        (
            REFUSALS[5],
            "a domain carrying a semicolon",
            Cookie::new("a", "1").domain("x;y"),
        ),
        (
            REFUSALS[5],
            "a domain carrying a space",
            Cookie::new("a", "1").domain("example .com"),
        ),
        (
            REFUSALS[5],
            "a domain carrying an underscore",
            Cookie::new("a", "1").domain("exa_mple.com"),
        ),
        (
            REFUSALS[5],
            "an empty domain",
            Cookie::new("a", "1").domain(""),
        ),
        (
            REFUSALS[5],
            "a domain that is a dot",
            Cookie::new("a", "1").domain("."),
        ),
        (
            REFUSALS[5],
            "a domain with an empty label",
            Cookie::new("a", "1").domain("example..com"),
        ),
        (
            REFUSALS[5],
            "a domain with a label over 63 octets",
            Cookie::new("a", "1").domain(format!("{long_label}.com")),
        ),
        (
            REFUSALS[5],
            "a domain label starting with a hyphen",
            Cookie::new("a", "1").domain("-example.com"),
        ),
        (
            REFUSALS[5],
            "a domain label ending with a hyphen",
            Cookie::new("a", "1").domain("example-.com"),
        ),
    ]
}

/// Every case [`refused`] holds renders no field, and the cases are counted
/// against the refusals.
///
/// Refusing rather than escaping: a value carrying `;` would silently become an
/// *attribute* rather than part of the value, which is a cookie the service
/// believes it set and a different one the client stored.
///
/// The count is of `return None` in `encode`'s own source, each of which is a
/// refusal [`REFUSALS`] names, so a refusal added there without a case here
/// fails rather than passing on the cases that already exist.
#[test]
fn every_refusal_has_a_case() {
    let cases = refused();
    for (refusal, description, cookie) in &cases {
        assert_eq!(
            rendered(cookie),
            None,
            "{description} must not render a field ({refusal})"
        );
    }

    let covered: std::collections::BTreeSet<&str> =
        cases.iter().map(|(refusal, ..)| *refusal).collect();
    assert_eq!(covered, std::collections::BTreeSet::from(REFUSALS));

    let source = include_str!("../cookie.rs");
    let encode = source
        .split_once("pub fn encode(")
        .and_then(|(_, rest)| rest.split_once("\n    }\n"))
        .map(|(body, _)| body)
        .expect("`encode` is declared in `cookie.rs`");
    assert_eq!(
        encode.matches("return None").count(),
        REFUSALS.len(),
        "`encode` refuses in a place `REFUSALS` does not name; name it and give it a case"
    );
}

/// The control for each grammar [`refused`] reaches: the same shapes, legal,
/// at the edge of each clause where the clause has one.
#[test]
fn each_refused_grammar_admits_its_legal_edge() {
    let longest_label = "a".repeat(63);
    for legal in [
        Cookie::new("a-b_c", "1"),
        Cookie::new("a", "abc-123_%"),
        Cookie::new("a", "!#$%&'()*+-./:<=>?@[]^_`{|}~"),
        Cookie::new("a", "1").path("/x/y"),
        Cookie::new("a", "1").path("/x y"),
        Cookie::new("a", "1").domain("sub.example.com"),
        Cookie::new("a", "1").domain(".example.com"),
        Cookie::new("a", "1").domain("ex-ample.com"),
        Cookie::new("a", "1").domain(format!("{longest_label}.com")),
        Cookie::new("__Host-a", "1"),
        Cookie::new("__Host-a", "1").path("/"),
        Cookie::new("__Secure-a", "1"),
        // Name and value together at the 4096-octet limit, which is legal.
        Cookie::new("a", "x".repeat(4095)),
    ] {
        assert!(
            rendered(&legal).is_some(),
            "a legal cookie was refused: {legal:?}"
        );
    }
}

/// An empty value is legal and is not the same as no cookie.
#[test]
fn an_empty_value_is_a_value() {
    assert_eq!(rendered(&Cookie::new("a", "")).as_deref(), Some("a="));
}

/// `draft-ietf-httpbis-rfc6265bis-22` section 4.1.3.1: a `__Secure-` cookie
/// "will have been set with a Secure attribute".
///
/// The requirement is on the user agent, which discards the whole `Set-Cookie`
/// otherwise — so a server that omits `Secure` here believes it set a cookie
/// the client never stored. The same failure `SameSite=None` is already
/// upgraded to avoid, and the same remedy.
#[test]
fn a_secure_prefixed_cookie_carries_secure_without_being_asked() {
    let rendered = rendered(&Cookie::new("__Secure-session", "abc")).expect("a legal cookie");
    assert!(rendered.contains("; Secure"), "{rendered}");
}

/// Section 4.1.3.2: a `__Host-` cookie "will have been set with a Secure
/// attribute, a Path attribute with a value of /, and no Domain attribute".
///
/// `Secure` and `Path=/` are supplied when absent, because neither narrows what
/// the caller asked for. A `Domain`, or a `Path` naming something other than
/// `/`, is refused instead: dropping the first would widen the cookie beyond
/// one host and rewriting the second would widen it beyond one subtree, and
/// silently widening a cookie's scope is worse than not setting it.
#[test]
fn a_host_prefixed_cookie_is_completed_rather_than_left_to_be_discarded() {
    let rendered = rendered(&Cookie::new("__Host-session", "abc")).expect("a legal cookie");

    assert!(rendered.contains("; Secure"), "{rendered}");
    assert!(rendered.contains("; Path=/"), "{rendered}");
    assert!(!rendered.contains("Domain="), "{rendered}");
}

/// Section 5.4: "UAs MUST match cookie name prefixes case-insensitively",
/// and section 5.7 steps 20 and 21 apply both prefixes on "a case-insensitive
/// match".
///
/// Section 4.1.3's case-sensitive wording describes what a server may rely on
/// when reading a name, not what a user agent discards. The encoder exists to
/// keep a cookie from being discarded, so it follows the user agent: a
/// `__secure-` cookie without `Secure` is dropped exactly as a `__Secure-` one
/// is.
#[test]
fn a_secure_prefix_in_any_case_carries_secure() {
    for name in ["__secure-session", "__SECURE-session", "__sEcUrE-session"] {
        let rendered = rendered(&Cookie::new(name, "abc")).expect("a legal cookie");
        assert!(rendered.contains("; Secure"), "{rendered}");
    }
}

/// The `__Host-` prefix in any case is completed with `Secure` and `Path=/`,
/// as section 5.7 step 21 requires of it.
#[test]
fn a_host_prefix_in_any_case_is_completed() {
    for name in ["__host-session", "__HOST-session", "__hOsT-session"] {
        let rendered = rendered(&Cookie::new(name, "abc")).expect("a legal cookie");
        assert!(rendered.contains("; Secure"), "{rendered}");
        assert!(rendered.contains("; Path=/"), "{rendered}");
    }
}

/// The `__Host-` refusals apply in any case, since a user agent discards a
/// `__host-` cookie naming a `Domain` or a non-root `Path` all the same.
#[test]
fn a_host_prefix_in_any_case_refuses_a_domain_or_a_narrower_path() {
    for name in ["__host-session", "__HOST-session"] {
        assert_eq!(
            rendered(&Cookie::new(name, "abc").domain("site.example")),
            None,
            "{name} with a Domain"
        );
        assert_eq!(
            rendered(&Cookie::new(name, "abc").path("/admin")),
            None,
            "{name} with a narrower Path"
        );
    }
}

/// A name that only shares the prefix's letters short of its `-` is an
/// ordinary name, in any case.
#[test]
fn a_name_short_of_a_prefix_is_an_ordinary_name() {
    for name in ["__host", "__Hostname", "__secure", "__SECUREx"] {
        let rendered = rendered(&Cookie::new(name, "abc")).expect("a legal cookie");
        assert!(!rendered.contains("Secure"), "{rendered}");
        assert!(!rendered.contains("Path="), "{rendered}");
    }
}
