use kynos_openapi::Method;

use super::{anchor, expressible, wire_methods};

/// A pattern is anchored at the literal segments before its first variable,
/// and only when there is a variable past them.
#[test]
fn a_pattern_is_anchored_at_the_literal_before_its_first_variable() {
    assert_eq!(anchor("/static/{*path}").as_deref(), Some("/static"));
    assert_eq!(anchor("/api/v1/{id}/raw").as_deref(), Some("/api/v1"));

    // Wholly literal: nothing varies past it, so there is no anchor.
    assert_eq!(anchor("/health"), None);
    assert_eq!(anchor("/api/v1"), None);

    // A variable in the first segment leaves only the root, which anchors
    // nothing.
    assert_eq!(anchor("/{id}"), None);
}

/// The empty segment before a leading `/` is the root, so a pattern that is
/// not rooted has no literal prefix to anchor at.
#[test]
fn an_unrooted_pattern_has_no_anchor() {
    assert_eq!(anchor("static/{*path}"), None);
    assert_eq!(anchor("{id}/raw"), None);
}

/// Only a catch-all, or a segment holding two variables, has no path template
/// equivalent.
#[test]
fn a_pattern_is_expressible_unless_it_catches_all_or_doubles_a_segment() {
    assert!(expressible("/health"));
    assert!(expressible("/users/{id}"));
    assert!(expressible("/users/{id}/posts/{post}"));
    assert!(expressible("/files/{name}.json"));

    assert!(!expressible("/static/{*path}"));
    assert!(!expressible("/files/{name}.{ext}"));
}

/// A method given twice is served, and recorded, once.
#[test]
fn a_repeated_method_is_served_once() {
    let (served, unmodelled) = wire_methods([
        crate::http::Method::GET,
        crate::http::Method::POST,
        crate::http::Method::GET,
    ]);

    assert_eq!(served, [Method::Get, Method::Post]);
    assert!(unmodelled.is_empty());
}
