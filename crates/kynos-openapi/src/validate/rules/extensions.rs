//! The `x-` naming rule for specification extensions.

use crate::{
    model::extensions::Extensions,
    validate::violation::{SpecError, Violation},
};

pub(in crate::validate) fn check_extensions(
    location: &str,
    extensions: &Extensions,
    violations: &mut Vec<Violation>,
) {
    for name in extensions.0.keys() {
        if !Extensions::is_valid_name(name) {
            violations.push(Violation::warning(
                location,
                SpecError::InvalidExtensionName { name: name.clone() },
            ));
        }
    }
}
