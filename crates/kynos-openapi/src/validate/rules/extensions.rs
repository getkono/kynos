//! The reserved-prefix rule for specification extensions.
//!
//! A name without the `x-` prefix is no extension at all, and is an error under
//! either version by the unrecognised-field walk; only the reserved `x-oai-`
//! and `x-oas-` prefixes are left to warn about here. The same walk finds
//! them, so every object that holds an `extensions` map is checked, each at
//! its own pointer (`references/3.1.2.md:4206`, `references/3.2.0.md:4747`).

use crate::{
    emit::downgrade::unrecognised::Field,
    validate::violation::{SpecError, Violation},
};

/// Warns of each reserved name the walk found, at the object holding it.
pub(in crate::validate) fn check_extensions(reserved: Vec<Field>, violations: &mut Vec<Violation>) {
    for field in reserved {
        violations.push(Violation::warning(
            field.holder,
            SpecError::InvalidExtensionName { name: field.name },
        ));
    }
}
