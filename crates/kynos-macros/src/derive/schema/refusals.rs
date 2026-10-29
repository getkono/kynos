//! What the `Schema` derive refuses before it emits anything.
//!
//! Each rule refuses a form with no schema true of the wire in both
//! directions; the expansion reads only inputs every rule has passed.

mod grammar;
mod naming;
mod object_keys;
mod recursion;
mod skips;
mod wire_form;

use syn::DeriveInput;

use super::container::Container;

/// Runs every refusal against the input and the one reading of its
/// `container`, in the order the later ones rely on ("refused before this
/// runs"). The `ParamValue` derive runs them too
/// ([`variant_wire_names`](super::members::variant_wire_names)), since it
/// reads the names they guard.
pub(super) fn check(input: &DeriveInput, container: &Container) -> syn::Result<()> {
    wire_form::reject_container_conversions(input)?;
    wire_form::reject_untagged(input, container)?;
    skips::reject_unread_variant(input, container)?;
    skips::reject_empty_tagged_enum(input, container)?;
    naming::reject_split_rename_all(input, container)?;
    naming::reject_split_rename(input, container)?;
    naming::reject_shadowed_variant(input, container)?;
    wire_form::reject_wire_form_overrides(input, container)?;
    wire_form::reject_catch_all(input, container)?;
    wire_form::reject_transparent_without_one_field(input)?;
    skips::reject_read_required_skip(input, container)?;
    object_keys::reject_contradicted_closure(input, container)?;
    object_keys::reject_closed_tagged_struct(input, container)?;
    object_keys::reject_field_named_as_tag(input, container)?;
    object_keys::reject_unread_field_in_closed_object(input, container)?;
    skips::reject_one_way_member_skip(input, container)?;
    skips::reject_skipped_adjacent_payload(input, container)?;
    grammar::check_constraints(input, container)?;
    recursion::reject_recursive_generic(input, container)
}
