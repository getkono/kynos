//! A member whose absence serde cannot tell from `null`.

use syn::{DeriveInput, Fields, spanned::Spanned};

use crate::derive::{
    common::wrapped,
    schema::{
        attributes::{described_members, is_flattened, serde_flag},
        container::Container,
        members::described_groups,
    },
};

/// A `Nullable` named field serde reads with no `#[serde(default)]`, on the
/// field or its struct, is refused.
///
/// serde hands a missing member to its type as `null`, so without a default
/// serde reads a member a patch left out as one sent to clear it. A default
/// fills it with `Nullable::Absent` instead, and leaves it out of `required`.
/// Recognised syntactically, as `Option` is. A transparent struct's member and
/// a positional one are never missing in that way: serde reads the first
/// whole, and refuses a short array before the second could be.
pub(super) fn reject_undefaulted_nullable(
    input: &DeriveInput,
    container: &Container,
) -> syn::Result<()> {
    if container.transparent || container.default {
        return Ok(());
    }

    let undefaulted = described_groups(input, container)
        .into_iter()
        .filter(|fields| matches!(fields, Fields::Named(_)))
        .flat_map(described_members)
        .find(|field| {
            wrapped(&field.ty, "Nullable").is_some()
                && !is_flattened(field)
                && !serde_flag(&field.attrs, &["default"])
        });
    let Some(field) = undefaulted else {
        return Ok(());
    };
    Err(syn::Error::new(
        field.ty.span(),
        "serde hands a member left out of the document to its type as `null`, so without \
         `#[serde(default)]` this `Nullable` reads as `Nullable::Null` when the member is left \
         out, and a patch that leaves it alone would clear it. Add `#[serde(default)]` on the \
         field or its struct, which fills it with `Nullable::Absent` and leaves it out of \
         `required`",
    ))
}
