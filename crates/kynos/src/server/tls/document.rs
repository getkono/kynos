//! Declaring mutual TLS in the description.
//!
//! Requiring a client certificate changes who may call every operation, so it
//! is the one listener setting that reaches the OpenAPI document. Enabling mTLS
//! and documenting it are the same act.

use crate::{
    security::{SecurityScheme, schemes::MutualTls},
    server::error::ServerError,
};

/// The component key the scheme is registered under: the name a `MutualTls`
/// guard registers it under, so a guard's entry is the same scheme rather than
/// a conflict.
pub(in crate::server) const MUTUAL_TLS_NAME: &str = <MutualTls as SecurityScheme>::NAME;

pub(in crate::server) fn apply_mutual_tls(
    document: &mut kynos_openapi::Document,
) -> std::result::Result<(), ServerError> {
    use kynos_openapi::{ComponentName, RefOr, SecurityRequirement, SecurityScheme};

    fn require_mutual_tls(requirements: &mut Vec<SecurityRequirement>) {
        if requirements.is_empty() {
            requirements.push(SecurityRequirement::scheme(MUTUAL_TLS_NAME));
        } else {
            for requirement in requirements {
                requirement.0.entry(MUTUAL_TLS_NAME.to_owned()).or_default();
            }
        }
    }

    let scheme = SecurityScheme::mutual_tls();
    match document.components.security_schemes.get(MUTUAL_TLS_NAME) {
        Some(RefOr::Item(existing)) if existing == &scheme => {}
        Some(_) => return Err(ServerError::MutualTlsConflict),
        None => {
            let name = ComponentName::new(MUTUAL_TLS_NAME)
                .expect("the built-in mutual TLS component name is valid");
            document.components.insert_security_scheme(&name, scheme);
        }
    }

    require_mutual_tls(&mut document.security);
    for operation in document
        .paths
        .items
        .values_mut()
        .flat_map(kynos_openapi::PathItem::operations_mut)
    {
        if let Some(requirements) = &mut operation.security {
            require_mutual_tls(requirements);
        }
    }

    Ok(())
}
