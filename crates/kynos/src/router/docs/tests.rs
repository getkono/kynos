//! What the built-in pages do with the two values substituted into them.
//!
//! Sweeps over [`page::SHIPPED`] rather than cases per renderer: the set is
//! finite and written down once, so enumerating it is total where a case per
//! page a reader happened to think of is a sample. A page added to the table
//! without a rule holding for it fails these.

use super::{Docs, page};

/// Every page, rendered with the values a caller would set.
fn rendered(description_url: &str, title: &str) -> Vec<(&'static str, String)> {
    page::SHIPPED
        .iter()
        .map(|(name, template)| (*name, page::render(template, description_url, title)))
        .collect()
}

#[test]
fn every_shipped_page_points_at_the_configured_description() {
    for (name, html) in rendered("/v1/openapi.json", "Example API") {
        assert!(
            html.contains("/v1/openapi.json"),
            "the {name} page does not fetch the description it was given",
        );
        assert!(
            !html.contains("{{description_url}}"),
            "the {name} page left its token unsubstituted",
        );
    }
}

#[test]
fn no_shipped_page_hardcodes_the_default_description_path() {
    // Rendered with a path sharing no substring with the default, so a
    // surviving `openapi.json` is a literal the template carries rather than
    // the one just substituted. The token is the only thing that moves under a
    // `nest`, so a second mention anywhere is a page that breaks under one.
    for (name, html) in rendered("/spec.yaml", "Example API") {
        assert!(
            !html.contains("openapi.json"),
            "the {name} page hardcodes the default description path",
        );
    }
}

#[test]
fn every_shipped_page_carries_the_configured_title() {
    for (name, html) in rendered("/openapi.json", "Widgets API") {
        assert!(
            html.contains("Widgets API"),
            "the {name} page does not show the title it was given",
        );
        assert!(
            !html.contains("{{title}}"),
            "the {name} page left its token unsubstituted",
        );
    }
}

#[test]
fn every_shipped_page_escapes_a_path_that_is_legal_but_hostile_to_its_syntax() {
    // `'` is an RFC 3986 sub-delimiter, so this is a path `PathTemplate`
    // accepts -- and the one that would close a single-quoted JavaScript
    // string. Substituting a whole JSON string literal is what keeps it inside
    // one.
    for (name, html) in rendered("/it's", "Example API") {
        assert!(
            html.contains("\"/it's\""),
            "the {name} page did not substitute the path as a JSON string: {html}",
        );
        assert!(
            !html.contains("'/it's'"),
            "the {name} page put the path in a quote the path itself can close",
        );
    }
}

#[test]
fn every_shipped_page_escapes_a_title_that_is_hostile_to_markup() {
    for (name, html) in rendered("/openapi.json", "Fish & <Chips>") {
        assert!(
            html.contains("Fish &amp; &lt;Chips&gt;"),
            "the {name} page did not escape its title",
        );
        assert!(
            !html.contains("<Chips>"),
            "the {name} page left a tag in its title",
        );
    }
}

#[test]
fn a_custom_page_naming_no_token_is_served_as_written() {
    // The control for every sweep above. Without it they pass against an
    // implementation that rewrites whatever page it is handed.
    let written = "<!doctype html><p>hi</p>";

    assert_eq!(
        page::render(written, "/openapi.json", "Example API"),
        written
    );
}

#[test]
fn a_custom_page_gets_the_same_substitution_the_shipped_ones_do() {
    // A vendored copy of a built-in is the usual custom page, so losing
    // substitution there would lose it exactly where it is needed.
    let rendered = page::render("<a href={{description_url}}>{{title}}</a>", "/spec", "API");

    assert_eq!(rendered, r#"<a href="/spec">API</a>"#);
}

#[test]
fn the_defaults_are_the_documented_ones() {
    // A default stated only in rustdoc is a default nothing checks.
    let docs = Docs::scalar();

    assert_eq!(docs.at.as_str(), "/docs");
    assert_eq!(docs.description_at.as_str(), "/openapi.json");
    assert_eq!(docs.operation_id_prefix, "docs");
    assert_eq!(docs.title, None);
}
