//! Security Scheme rules: each OAuth flow declares the URLs its flow requires.
//!
//! The root requirements' names are checked with the document rules, by the
//! predicate an operation's requirements share.

use crate::{
    model::{
        document::Document,
        security::{
            SecurityScheme,
            oauth::{OAuthFlow, OAuthFlows},
        },
    },
    validate::violation::{SpecError, Violation, pointer_token},
};

/// Checks every OAuth flow of every Security Scheme defined in `components`.
///
/// A `$ref` is not followed: the scheme it names is checked where it is
/// defined.
pub(in crate::validate) fn check_security_schemes(
    document: &Document,
    violations: &mut Vec<Violation>,
) {
    for (name, scheme) in &document.components.security_schemes {
        if let Some(SecurityScheme::OAuth2 { flows, .. }) = scheme.as_item() {
            check_flows(
                &format!("#/components/securitySchemes/{}/flows", pointer_token(name)),
                flows,
                violations,
            );
        }
    }
}

/// A URL field of the OAuth Flow Object: its wire name, and whether a flow
/// declares it.
type Url = (&'static str, fn(&OAuthFlow) -> bool);

const AUTHORIZATION_URL: Url = ("authorizationUrl", |flow| flow.authorization_url.is_some());
const TOKEN_URL: Url = ("tokenUrl", |flow| flow.token_url.is_some());
#[cfg(feature = "openapi32")]
const DEVICE_AUTHORIZATION_URL: Url = ("deviceAuthorizationUrl", |flow| {
    flow.device_authorization_url.is_some()
});

/// Reports each URL a declared flow omits that the "Applies To" column of the
/// OAuth Flow Object's fixed fields makes REQUIRED for that flow.
fn check_flows(location: &str, flows: &OAuthFlows, violations: &mut Vec<Violation>) {
    let declared: &[(&'static str, &Option<OAuthFlow>, &[Url])] = &[
        ("implicit", &flows.implicit, &[AUTHORIZATION_URL]),
        ("password", &flows.password, &[TOKEN_URL]),
        ("clientCredentials", &flows.client_credentials, &[TOKEN_URL]),
        (
            "authorizationCode",
            &flows.authorization_code,
            &[AUTHORIZATION_URL, TOKEN_URL],
        ),
        #[cfg(feature = "openapi32")]
        (
            "deviceAuthorization",
            &flows.device_authorization,
            &[DEVICE_AUTHORIZATION_URL, TOKEN_URL],
        ),
    ];
    for &(flow_name, flow, required) in declared {
        let Some(flow) = flow else { continue };
        for &(field, declares) in required {
            if !declares(flow) {
                violations.push(Violation::error(
                    format!("{location}/{flow_name}"),
                    SpecError::MissingOAuthFlowUrl {
                        flow: flow_name,
                        field,
                    },
                ));
            }
        }
    }
}
