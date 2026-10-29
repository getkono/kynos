//! One module per family of specification rule.
//!
//! Each contributes to [`Validator`](crate::validate::Validator) as an inherent
//! `impl` block or free functions [`Validator::validate`] calls.

pub(in crate::validate) mod content;
pub(in crate::validate) mod discriminator;
pub(in crate::validate) mod document;
pub(in crate::validate) mod extensions;
pub(in crate::validate) mod opaque;
pub(in crate::validate) mod operations;
pub(in crate::validate) mod parameters;
pub(in crate::validate) mod paths;
pub(in crate::validate) mod schemas;
pub(in crate::validate) mod security;

/// The name `reference` gives a component in `section`, where it is a
/// same-document pointer into `#/components/<section>/`, as written.
///
/// What remains is returned undecoded, so a lookup by it finds a component
/// only under exactly that spelling.
pub(in crate::validate) fn component_name<'r>(
    reference: &'r str,
    section: &str,
) -> Option<&'r str> {
    reference
        .strip_prefix("#/components/")?
        .strip_prefix(section)?
        .strip_prefix('/')
}

/// The same, refusing a name holding `/`, `~` or `%`: such a pointer is
/// escaped or reaches below the component, and no valid component name
/// needs any of the three.
pub(in crate::validate) fn literal_component_name<'r>(
    reference: &'r str,
    section: &str,
) -> Option<&'r str> {
    component_name(reference, section).filter(|name| !name.contains(['/', '~', '%']))
}
