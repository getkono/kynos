use std::time::Duration;

use super::{Cookie, SameSite};

/// What a cookie renders to, or `None`.
fn rendered(cookie: &Cookie) -> Option<String> {
    cookie
        .encode()
        .map(|value| value.to_str().expect("a printable field").to_owned())
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
