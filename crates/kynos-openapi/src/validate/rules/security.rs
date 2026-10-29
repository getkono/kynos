//! Security rules: an `apiKey` is carried in a query, header or cookie, each
//! OAuth flow declares the URLs its flow requires, and each requirement names
//! a scheme.
//!
//! The root requirements are checked here; an operation's are checked with
//! the rest of its rules, by the same predicate.

use std::collections::HashSet;

use crate::{
    model::{
        document::Document,
        parameter::ParameterIn,
        security::{
            SecurityScheme,
            oauth::{OAuthFlow, OAuthFlows},
        },
    },
    pointer::At,
    validate::{
        Validator,
        rules::literal_component_name,
        violation::{SpecError, Violation},
    },
    visit,
};

impl Validator {
    /// Whether the security requirement key `name` corresponds to a security
    /// scheme, given the component names in `declared`.
    ///
    /// A declared component name takes precedence under either version. 3.2
    /// also admits a scheme's URI, read by the predicate `Document::emit`
    /// refuses 3.1 on; 3.1 admits only a component name. A same-document
    /// pointer into the Security Schemes resolves here, so it must land on a
    /// declared one; a URI into another document cannot be followed, and
    /// passes. A pointer name escaped with `~` or `%` passes undecoded, since
    /// no valid component name needs either.
    pub(in crate::validate) fn names_a_scheme(self, name: &str, declared: &HashSet<&str>) -> bool {
        if declared.contains(name) {
            return true;
        }
        if !self.version.supports_3_2() || !crate::emit::downgrade::names_a_scheme_by_uri(name) {
            return false;
        }
        literal_component_name(name, "securitySchemes")
            .is_none_or(|component| declared.contains(component))
    }

    /// Checks that every name in the root `security` list resolves, by the
    /// rule an operation's requirements are held to.
    pub(in crate::validate) fn check_security(
        self,
        document: &Document,
        declared: &HashSet<&str>,
        violations: &mut Vec<Violation>,
    ) {
        for (index, requirement) in document.security.iter().enumerate() {
            for name in requirement.0.keys() {
                if !self.names_a_scheme(name, declared) {
                    violations.push(Violation::error(
                        format!("#/security/{index}"),
                        SpecError::UnknownSecurityScheme { name: name.clone() },
                    ));
                }
            }
        }
    }
}

/// Checks every Security Scheme defined in `components`: where an `apiKey` is
/// carried, and every OAuth flow.
///
/// A `$ref` is not followed: the scheme it names is checked where it is
/// defined.
pub(in crate::validate) fn check_security_schemes(
    document: &Document,
    violations: &mut Vec<Violation>,
) {
    visit::inline_items(
        At::ROOT.key("components").key("securitySchemes"),
        &document.components.security_schemes,
        |at, scheme| match scheme {
            SecurityScheme::ApiKey { location, .. } => {
                if !matches!(
                    location,
                    ParameterIn::Query | ParameterIn::Header | ParameterIn::Cookie
                ) {
                    violations.push(Violation::error(
                        at.key("in").pointer(),
                        SpecError::IllegalApiKeyLocation {
                            location: location.as_str(),
                        },
                    ));
                }
            }
            SecurityScheme::OAuth2 { flows, .. } => {
                check_flows(&at.key("flows").pointer(), flows, violations);
            }
            _ => {}
        },
    );
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
