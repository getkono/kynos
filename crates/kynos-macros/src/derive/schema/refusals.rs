//! What the `Schema` derive refuses before it emits anything.
//!
//! Each rule names a form whose declaration predicts no schema true of the
//! wire in both directions, a `#[schema(...)]` outside the grammar, or a
//! generic type whose schema would never end, and the expansion reads only
//! inputs every rule here has passed. The rules are grouped by the serde form
//! they read, the last by none; [`check`] holds the order across the groups.

mod grammar;
mod naming;
mod object_keys;
mod recursion;
mod skips;
mod wire_form;

use syn::DeriveInput;

/// Runs every refusal, in the order the later ones rely on.
///
/// Several rules say a form is refused "before this runs" by another; the
/// order here is what makes that true.
pub(super) fn check(input: &DeriveInput) -> syn::Result<()> {
    wire_form::reject_container_conversions(input)?;
    wire_form::reject_untagged(input)?;
    skips::reject_unread_variant(input)?;
    naming::reject_split_rename_all(input)?;
    naming::reject_split_rename(input)?;
    naming::reject_shadowed_variant(input)?;
    wire_form::reject_wire_form_overrides(input)?;
    wire_form::reject_catch_all(input)?;
    wire_form::reject_transparent_without_one_field(input)?;
    skips::reject_read_required_skip(input)?;
    object_keys::reject_contradicted_closure(input)?;
    object_keys::reject_closed_tagged_struct(input)?;
    object_keys::reject_field_named_as_tag(input)?;
    object_keys::reject_unread_field_in_closed_object(input)?;
    skips::reject_one_way_member_skip(input)?;
    skips::reject_skipped_adjacent_payload(input)?;
    grammar::check_constraints(input)?;
    recursion::reject_recursive_generic(input)
}
