//! The reserved-prefix rule for specification extensions.
//!
//! A name without the `x-` prefix is no extension at all, and is an error under
//! either version by the unrecognised-field walk; only the reserved `x-oai-`
//! and `x-oas-` prefixes are left to warn about here.

use crate::{
    model::extensions::{EXTENSION_PREFIX, Extensions},
    validate::violation::{SpecError, Violation},
};

pub(in crate::validate) fn check_extensions(
    location: &str,
    extensions: &Extensions,
    violations: &mut Vec<Violation>,
) {
    for name in extensions.0.keys() {
        if name.starts_with(EXTENSION_PREFIX) && !Extensions::is_valid_name(name) {
            violations.push(Violation::warning(
                location,
                SpecError::InvalidExtensionName { name: name.clone() },
            ));
        }
    }
}
