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
#[test]
fn every_attribute_builder_is_covered_by_the_sweep() {
    let source = include_str!("../cookie.rs");

    let declared: std::collections::BTreeSet<&str> = source
        .lines()
        .filter_map(|line| line.trim().strip_prefix("pub fn "))
        .filter(|rest| rest.contains("(mut self"))
        .filter_map(|rest| rest.split('(').next())
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
        "an attribute builder is not exercised by `every_attribute_reaches_the_field_in_order`;          add it to that cookie and to this set, or the field it renders is asserted nowhere"
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

/// The other two values imply nothing.
#[test]
fn the_other_same_site_values_imply_no_attribute() {
    for value in [SameSite::Strict, SameSite::Lax] {
        let rendered =
            rendered(&Cookie::new("a", "1").same_site(value)).expect("a representable cookie");
        assert!(!rendered.contains("Secure"), "{value:?}: {rendered}");
    }
}

/// One case per way `encode` refuses, counted against the refusals.
///
/// Refusing rather than escaping: a value carrying `;` would silently become an
/// *attribute* rather than part of the value, which is a cookie the service
/// believes it set and a different one the client stored.
#[test]
fn every_refusal_has_a_case() {
    let cases: &[(&str, Cookie)] = &[
        ("a name outside the token grammar", Cookie::new("a b", "1")),
        ("an empty name", Cookie::new("", "1")),
        (
            "a value carrying a semicolon",
            Cookie::new("a", "1; Path=/"),
        ),
        ("a value carrying a comma", Cookie::new("a", "1,2")),
        ("a value carrying a quote", Cookie::new("a", "\"1\"")),
        ("a value carrying a backslash", Cookie::new("a", "1\\2")),
        ("a value carrying a space", Cookie::new("a", "1 2")),
        (
            "a path carrying a semicolon",
            Cookie::new("a", "1").path("/x;y"),
        ),
        (
            "a domain carrying a semicolon",
            Cookie::new("a", "1").domain("x;y"),
        ),
        (
            "a domain carrying a space",
            Cookie::new("a", "1").domain("example .com"),
        ),
        (
            "a __Host- cookie with a Domain",
            Cookie::new("__Host-a", "1").domain("example.com"),
        ),
        (
            "a __Host- cookie scoped to a path other than /",
            Cookie::new("__Host-a", "1").path("/admin"),
        ),
        (
            "a name and value over 4096 octets",
            Cookie::new("a", "x".repeat(4096)),
        ),
    ];

    for (description, cookie) in cases {
        assert_eq!(
            rendered(cookie),
            None,
            "{description} must not render a field"
        );
    }

    // The control for each grammar: the same shapes, legal.
    for legal in [
        Cookie::new("a-b_c", "1"),
        Cookie::new("a", "abc-123_%"),
        Cookie::new("a", "1").path("/x/y"),
        Cookie::new("a", "1").domain("sub.example.com"),
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

/// The prefixes are matched case-sensitively, per "a case-sensitive match for
/// the string".
///
/// `__host-` is an ordinary name and carries no requirement, so completing it
/// would be inventing an attribute the caller did not ask for.
#[test]
fn a_prefix_in_the_wrong_case_is_an_ordinary_name() {
    let rendered = rendered(&Cookie::new("__host-session", "abc")).expect("a legal cookie");

    assert!(!rendered.contains("Secure"), "{rendered}");
    assert!(!rendered.contains("Path="), "{rendered}");
}
