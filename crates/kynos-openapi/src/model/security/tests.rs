use crate::model::security::SecurityScheme;

/// Each constructor builds the variant its name claims, in the spelling the
/// specification gives it.
///
/// This counts the *spellings*, which are what a caller writes and what a
/// reader of the emitted document sees. Every variant has a constructor, so
/// the rows reach each variant's `type`; three of the five differ from their
/// Rust name by more than case, and the three `apiKey` helpers differ from
/// each other only in a field.
#[test]
fn every_constructor_builds_what_it_names() {
    use crate::model::security::oauth::OAuthFlows;

    const SOURCE: &str = include_str!("mod.rs");

    let cases = [
        (
            SecurityScheme::http("negotiate", None),
            serde_json::json!({ "type": "http", "scheme": "negotiate" }),
        ),
        (
            SecurityScheme::bearer(Some("JWT".to_owned())),
            serde_json::json!({ "type": "http", "scheme": "bearer", "bearerFormat": "JWT" }),
        ),
        (
            SecurityScheme::basic(),
            serde_json::json!({ "type": "http", "scheme": "basic" }),
        ),
        (
            SecurityScheme::api_key_header("X-Api-Key"),
            serde_json::json!({ "type": "apiKey", "name": "X-Api-Key", "in": "header" }),
        ),
        (
            SecurityScheme::api_key_query("api_key"),
            serde_json::json!({ "type": "apiKey", "name": "api_key", "in": "query" }),
        ),
        (
            SecurityScheme::api_key_cookie("session"),
            serde_json::json!({ "type": "apiKey", "name": "session", "in": "cookie" }),
        ),
        (
            SecurityScheme::mutual_tls(),
            serde_json::json!({ "type": "mutualTLS" }),
        ),
        (
            SecurityScheme::oauth2(OAuthFlows::default()),
            serde_json::json!({ "type": "oauth2", "flows": {} }),
        ),
        (
            SecurityScheme::open_id_connect("https://example.com/.well-known"),
            serde_json::json!({
                "type": "openIdConnect",
                "openIdConnectUrl": "https://example.com/.well-known",
            }),
        ),
    ];

    for (scheme, expected) in &cases {
        assert_eq!(
            &serde_json::to_value(scheme).expect("serializable"),
            expected
        );
    }

    // Counted against the source, so a ninth constructor without a row fails
    // the build rather than joining a silent majority.
    let declared = SOURCE.matches("\n    pub fn ").count() - SOURCE.matches("(mut self").count();
    assert_eq!(
        cases.len(),
        declared,
        "`security/mod.rs` declares {declared} constructor(s) and {} have a row",
        cases.len()
    );
}

/// `with_oauth2_metadata_url` is a no-op on a scheme that has no such field.
///
/// Recorded rather than argued away. `SecurityScheme` is one enum and only
/// `oauth2` carries the URL, so the setter has nowhere to put it on the other
/// four; refusing would mean returning a `Result` from a builder, and panicking
/// would be worse. `#[derive(SecurityScheme)]` only ever emits the call in its
/// `oauth2` arm, so the branch is unreachable from generated code — this test
/// is what keeps that a fact on the record rather than an assumption.
#[cfg(feature = "openapi32")]
#[test]
fn a_metadata_url_set_on_a_scheme_that_cannot_carry_one_changes_nothing() {
    let untouched = SecurityScheme::basic().with_oauth2_metadata_url("https://example.com/meta");
    assert_eq!(untouched, SecurityScheme::basic());
}
