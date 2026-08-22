use crate::model::security::{SecurityScheme, requirement::SecurityRequirement};

#[test]
fn the_scheme_type_is_the_serde_tag() {
    let json = serde_json::to_string(&SecurityScheme::bearer(Some("JWT".to_owned())))
        .expect("serializable");
    assert!(json.contains(r#""type":"http""#));
    assert!(json.contains(r#""scheme":"bearer""#));
    assert!(json.contains(r#""bearerFormat":"JWT""#));
}

/// The `type` the specification gives each scheme.
///
/// An exhaustive match, so a scheme added to the enum stops this file
/// compiling until its wire spelling is written down. Three of the five differ
/// from the Rust name by more than case, which is the reason to check all of
/// them rather than one.
fn wire_tag(scheme: &SecurityScheme) -> &'static str {
    match scheme {
        SecurityScheme::ApiKey { .. } => "apiKey",
        SecurityScheme::Http { .. } => "http",
        SecurityScheme::MutualTls { .. } => "mutualTLS",
        SecurityScheme::OAuth2 { .. } => "oauth2",
        SecurityScheme::OpenIdConnect { .. } => "openIdConnect",
    }
}

#[test]
fn every_scheme_carries_the_type_the_specification_spells() {
    use crate::model::{
        parameter::ParameterIn,
        security::oauth::{OAuthFlow, OAuthFlows},
    };

    let schemes = [
        SecurityScheme::api_key_header("X-Api-Key"),
        SecurityScheme::basic(),
        SecurityScheme::mutual_tls(),
        SecurityScheme::oauth2(
            OAuthFlows::default()
                .with_client_credentials(OAuthFlow::new([("read".to_owned(), "Read".to_owned())])),
        ),
        SecurityScheme::open_id_connect("https://example.com/.well-known/openid-configuration"),
    ];

    // Every variant, not merely every constructor: `ApiKey` has one helper but
    // three locations, and the tag is a property of the variant.
    let mut seen: Vec<&str> = Vec::new();
    for scheme in &schemes {
        let value = serde_json::to_value(scheme).expect("serializable");
        assert_eq!(
            value.get("type").and_then(serde_json::Value::as_str),
            Some(wire_tag(scheme)),
            "{scheme:?} carried the wrong `type`"
        );
        seen.push(wire_tag(scheme));
    }

    seen.sort_unstable();
    seen.dedup();
    assert_eq!(
        seen.len(),
        5,
        "every scheme variant needs a case here, got {seen:?}"
    );

    // `apiKey` takes its location from a field rather than from the tag.
    let cookie = SecurityScheme::ApiKey {
        name: "session".to_owned(),
        location: ParameterIn::Cookie,
        description: None,
        #[cfg(feature = "openapi32")]
        deprecated: None,
        extensions: crate::model::extensions::Extensions::new(),
    };
    assert_eq!(
        serde_json::to_value(&cookie).expect("serializable"),
        serde_json::json!({ "type": "apiKey", "name": "session", "in": "cookie" })
    );
}

#[test]
fn mutual_tls_needs_no_further_configuration() {
    let json = serde_json::to_string(&SecurityScheme::mutual_tls()).expect("serializable");
    assert_eq!(json, r#"{"type":"mutualTLS"}"#);
}

#[test]
fn an_empty_requirement_means_anonymous_access() {
    assert!(SecurityRequirement::anonymous().is_anonymous());
    assert!(!SecurityRequirement::scheme("Bearer").is_anonymous());
}

#[test]
fn requirements_serialize_as_a_bare_map() {
    let json = serde_json::to_string(&SecurityRequirement::scoped("OAuth", ["read", "write"]))
        .expect("serializable");
    assert_eq!(json, r#"{"OAuth":["read","write"]}"#);
}

/// Each constructor builds the variant its name claims, in the spelling the
/// specification gives it.
///
/// The variants are counted elsewhere; this counts the *spellings*, which are
/// what a caller writes and what a reader of the emitted document sees. Three
/// of the eight differ from their Rust name by more than case, and the three
/// `apiKey` helpers differ from each other only in a field.
#[test]
fn every_constructor_builds_what_it_names() {
    use crate::model::security::oauth::OAuthFlows;

    const SOURCE: &str = include_str!("mod.rs");

    let cases = [
        (
            SecurityScheme::bearer(None),
            serde_json::json!({ "type": "http", "scheme": "bearer" }),
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

/// A description reaches whichever variant it was set on.
///
/// The setter matches all five, so a variant added without a `description`
/// field stops this compiling — which is the whole guard, and why there is no
/// row per variant here.
#[test]
fn a_description_reaches_every_variant() {
    let described = SecurityScheme::mutual_tls().with_description("A partner certificate");
    assert_eq!(
        serde_json::to_value(&described).expect("serializable"),
        serde_json::json!({ "type": "mutualTLS", "description": "A partner certificate" })
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
