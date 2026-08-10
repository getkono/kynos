//! Declaring mutual TLS in the description.
//!
//! Requiring a client certificate changes who may call every operation, so it
//! is the one listener setting that reaches the OpenAPI document. Enabling mTLS
//! and documenting it are the same act.

use crate::server::error::ServerError;

pub(in crate::server) const MUTUAL_TLS_NAME: &str = "MutualTls";

pub(in crate::server) fn apply_mutual_tls(
    document: &mut kynos_openapi::Document,
) -> std::result::Result<(), ServerError> {
    use kynos_openapi::{ComponentName, RefOr, SecurityRequirement, SecurityScheme};

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
    for path in document.paths.0.values_mut() {
        for operation in [
            &mut path.get,
            &mut path.put,
            &mut path.post,
            &mut path.delete,
            &mut path.options,
            &mut path.head,
            &mut path.patch,
            &mut path.trace,
        ] {
            if let Some(operation) = operation {
                if let Some(requirements) = &mut operation.security {
                    require_mutual_tls(requirements);
                }
            }
        }
        #[cfg(feature = "openapi32")]
        {
            if let Some(operation) = &mut path.query {
                if let Some(requirements) = &mut operation.security {
                    require_mutual_tls(requirements);
                }
            }
            for operation in path.additional_operations.values_mut() {
                if let Some(requirements) = &mut operation.security {
                    require_mutual_tls(requirements);
                }
            }
        }
    }

    fn require_mutual_tls(requirements: &mut Vec<SecurityRequirement>) {
        if requirements.is_empty() {
            requirements.push(SecurityRequirement::scheme(MUTUAL_TLS_NAME));
        } else {
            for requirement in requirements {
                requirement.0.entry(MUTUAL_TLS_NAME.to_owned()).or_default();
            }
        }
    }

    Ok(())
}
