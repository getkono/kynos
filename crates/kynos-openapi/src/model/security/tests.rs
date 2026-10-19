use crate::model::security::SecurityScheme;

/// The `type` the specification gives each variant, beside the set of them.
///
/// An exhaustive match, so a variant added to the enum stops this file
/// compiling until its wire spelling is written here and in `VARIANT_TAGS`,
/// and the constructor test then fails until a row reaches it.
fn wire_tag(scheme: &SecurityScheme) -> &'static str {
    match scheme {
        SecurityScheme::ApiKey { .. } => "apiKey",
        SecurityScheme::Http { .. } => "http",
        SecurityScheme::MutualTls { .. } => "mutualTLS",
        SecurityScheme::OAuth2 { .. } => "oauth2",
        SecurityScheme::OpenIdConnect { .. } => "openIdConnect",
    }
}

const VARIANT_TAGS: [&str; 5] = ["apiKey", "http", "mutualTLS", "oauth2", "openIdConnect"];

/// Each constructor builds the variant its name claims, in the spelling the
/// specification gives it.
///
/// This counts the *spellings*, which are what a caller writes and what a
/// reader of the emitted document sees, and names the variants the rows
/// reach against [`wire_tag`], so every variant has a constructor by
/// assertion rather than by intent. Three of the five differ from their Rust
/// name by more than case, and the three `apiKey` helpers differ from each
/// other only in a field.
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

    let mut reached = std::collections::BTreeSet::new();
    for (scheme, expected) in &cases {
        assert_eq!(
            &serde_json::to_value(scheme).expect("serializable"),
            expected
        );
        assert_eq!(
            expected.get("type").and_then(serde_json::Value::as_str),
            Some(wire_tag(scheme)),
            "{scheme:?} carried the wrong `type`"
        );
        reached.insert(wire_tag(scheme));
    }

    // Named rather than counted, so a failure says which variant no row reaches.
    assert_eq!(
        reached,
        VARIANT_TAGS.into_iter().collect(),
        "every `SecurityScheme` variant needs a constructor row"
    );

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
