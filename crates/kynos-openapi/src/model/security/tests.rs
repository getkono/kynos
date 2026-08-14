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
        SecurityScheme::OAuth2 {
            flows: Box::new(OAuthFlows {
                client_credentials: Some(OAuthFlow::new([("read".to_owned(), "Read".to_owned())])),
                ..OAuthFlows::default()
            }),
            description: None,
            #[cfg(feature = "openapi32")]
            oauth2_metadata_url: None,
            #[cfg(feature = "openapi32")]
            deprecated: None,
            extensions: crate::model::extensions::Extensions::new(),
        },
        SecurityScheme::OpenIdConnect {
            open_id_connect_url: "https://example.com/.well-known/openid-configuration".to_owned(),
            description: None,
            #[cfg(feature = "openapi32")]
            deprecated: None,
            extensions: crate::model::extensions::Extensions::new(),
        },
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
