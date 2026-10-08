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
        .map(|(name, shipped)| {
            (
                *name,
                page::render(shipped.template, description_url, title),
            )
        })
        .collect()
}

/// Every `<script ...>` opening tag in `html`, with the text up to its
/// `</script>`.
fn scripts(html: &str) -> Vec<(&str, &str)> {
    html.split("<script")
        .skip(1)
        .map(|rest| {
            let (tag, rest) = rest.split_once('>').expect("a closed opening tag");
            let (body, _) = rest.split_once("</script>").expect("a closed script");
            (tag, body)
        })
        .collect()
}

/// The CSP source expression naming `script` by its SHA-256.
///
/// The digest is `rustls`'s, which the dev-dependency graph already carries
/// through `ring`, rather than a hashing crate added for one assertion.
fn hash_source(script: &str) -> String {
    use rustls::{SupportedCipherSuite, crypto::ring::cipher_suite::TLS13_AES_128_GCM_SHA256};

    let SupportedCipherSuite::Tls13(suite) = TLS13_AES_128_GCM_SHA256 else {
        unreachable!("a TLS 1.3 suite")
    };
    let digest = suite.common.hash_provider.hash(script.as_bytes());

    format!("'sha256-{}'", base64(digest.as_ref()))
}

/// Standard, padded base64: the alphabet CSP hash sources use.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut encoded = String::new();
    for chunk in bytes.chunks(3) {
        let word = chunk.iter().enumerate().fold(0_u32, |word, (index, byte)| {
            word | u32::from(*byte) << (16 - 8 * index)
        });
        for index in 0..4 {
            if index <= chunk.len() {
                encoded.push(char::from(
                    ALPHABET[(word >> (18 - 6 * index) & 63) as usize],
                ));
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}

#[test]
fn the_base64_helper_matches_the_rfc_4648_vectors() {
    // `hash_source` is only as right as this, and nothing else checks it.
    for (input, expected) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(base64(input.as_bytes()), expected);
    }
}

#[test]
fn every_shipped_bundle_is_pinned_and_integrity_checked() {
    for (name, shipped) in page::SHIPPED {
        let external: Vec<_> = scripts(shipped.template)
            .into_iter()
            .filter(|(tag, _)| tag.contains("src="))
            .collect();
        assert!(!external.is_empty(), "the {name} page loads no bundle");

        for (tag, _) in external {
            let src = tag
                .split_once("src=\"")
                .and_then(|(_, rest)| rest.split_once('"'))
                .map(|(src, _)| src)
                .expect("a quoted src");

            assert!(
                tag.contains("integrity=\"sha384-"),
                "the {name} page loads {src} unchecked",
            );
            assert!(
                tag.contains("crossorigin=\"anonymous\""),
                "the {name} page loads {src} without CORS, so integrity is not enforced",
            );
            assert!(
                !src.contains("latest") && src.contains('@'),
                "the {name} page loads {src}, which names no exact version",
            );
            // Naming the file rather than the origin admits nothing else the
            // CDN hosts.
            assert!(
                shipped.policy.contains(&format!(" {src};")),
                "the {name} policy does not admit {src} by its full URL",
            );
        }
    }
}

#[test]
fn every_shipped_boot_script_is_admitted_by_its_hash_whatever_is_substituted() {
    // Rendered with hostile values, so a token that crept into a boot script
    // changes the hash and fails here instead of in a browser.
    for (name, html) in rendered("/it's", "Fish & <Chips>") {
        let policy = page::SHIPPED
            .iter()
            .find(|(shipped_name, _)| *shipped_name == name)
            .map(|(_, shipped)| shipped.policy)
            .expect("a shipped page");

        let inline: Vec<_> = scripts(&html)
            .into_iter()
            .filter(|(tag, _)| tag.is_empty())
            .collect();
        assert_eq!(inline.len(), 1, "the {name} page boots from one script");

        for (_, body) in inline {
            assert!(
                policy.contains(&hash_source(body)),
                "the {name} policy does not admit its boot script {body:?}",
            );
        }
    }
}

#[test]
fn every_shipped_page_runs_nothing_its_policy_does_not_name() {
    // Every script is a bundle, the boot script, or the data block -- which
    // the browser parses and never runs. Anything else would be a script the
    // policy blocks or one the sweeps above do not reach.
    for (name, shipped) in page::SHIPPED {
        for (tag, _) in scripts(shipped.template) {
            assert!(
                tag.is_empty() || tag.contains("src=") || tag.contains("type=\"application/json\""),
                "the {name} page carries a script the policy does not account for: {tag}",
            );
        }

        for directive in ["object-src 'none'", "base-uri 'none'"] {
            assert!(
                shipped.policy.contains(directive),
                "the {name} policy omits {directive}",
            );
        }
    }
}

#[test]
fn only_a_shipped_page_carries_a_policy() {
    // A custom page's loads are the application's to know, so it gets none.
    assert_eq!(Docs::scalar().policy, Some(page::SCALAR.policy));
    assert_eq!(Docs::redoc().policy, Some(page::REDOC.policy));
    assert_eq!(Docs::custom("<!doctype html>").policy, None);
    assert_eq!(Docs::custom(page::SCALAR.template).policy, None);
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
