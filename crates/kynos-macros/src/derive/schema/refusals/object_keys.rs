//! An object's key set: what `#[serde(deny_unknown_fields)]` closing it, and a
//! struct's own tag written beside its fields, leave no schema true of.

use syn::{Data, DeriveInput, Fields};

use crate::derive::schema::{
    Container, aliases,
    attributes::{
        described_members, field_name, is_flattened, is_skipped_both_ways, open_span, serde_flag,
        serde_key_span,
    },
    described_groups, unread_field_span, written_groups,
};

/// An object `#[serde(deny_unknown_fields)]` closes has no schema true of a
/// flattened open map.
///
/// serde refuses every key no field it reads names before a flattened map sees
/// it, so the map reads empty while serde writes its members. Checked on every
/// named field the schema describes, including inside a variant serde never
/// writes, since the object is closed on read alone. A `#[serde(transparent)]`
/// struct is its one field's value, with no object to close. An `alias` needs
/// no refusal: the object names every name serde reads a field under
/// ([`aliases`]).
pub(super) fn reject_contradicted_closure(input: &DeriveInput) -> syn::Result<()> {
    let container = Container::read(input);
    if !container.deny_unknown_fields || container.transparent {
        return Ok(());
    }

    let named = described_groups(input)
        .into_iter()
        .filter(|fields| matches!(fields, Fields::Named(_)))
        .flat_map(described_members);

    for field in named {
        // `open` on a field that is not flattened is `check_constraints`'
        // diagnostic, which names the actual mistake.
        if let Some(span) = open_span(field).filter(|_| is_flattened(field)) {
            return Err(syn::Error::new(
                span,
                "`#[schema(open)]` says this object admits members nothing names, but \
                 `#[serde(deny_unknown_fields)]` makes serde refuse every key its fields do not \
                 name before the map sees it, so serde reads the map empty and writes members it \
                 would refuse to read back. Drop `deny_unknown_fields` to keep the map, or drop \
                 the map",
            ));
        }
    }
    Ok(())
}

/// Whether serde writes a named struct's `#[serde(tag = "...")]`, and which:
/// every tagged named struct but a `#[serde(transparent)]` one, which serde
/// writes as its one field's value.
fn struct_tag<'a>(input: &DeriveInput, container: &'a Container) -> Option<&'a str> {
    let Data::Struct(data) = &input.data else {
        return None;
    };
    if container.transparent || !matches!(data.fields, Fields::Named(_)) {
        return None;
    }
    container.tag.as_deref()
}

/// A tagged struct `#[serde(deny_unknown_fields)]` closes has no true schema.
///
/// serde writes the tag beside the fields and never reads it back as one of
/// them, so the closed struct refuses the tag in every document it writes: a
/// schema naming the tag accepts what serde refuses, and one leaving it out
/// refuses what serde writes. A tuple or unit struct is left to serde, which
/// refuses the tag there itself.
pub(super) fn reject_closed_tagged_struct(input: &DeriveInput) -> syn::Result<()> {
    let container = Container::read(input);
    if struct_tag(input, &container).is_none() || !container.deny_unknown_fields {
        return Ok(());
    }
    let span = serde_key_span(&input.attrs, &["deny_unknown_fields"])
        .map_or_else(|| input.ident.span(), |(_, span)| span);
    Err(syn::Error::new(
        span,
        "`#[serde(tag = \"...\")]` makes serde write the tag beside this struct's fields, but \
         serde never reads it back as one of them, so `#[serde(deny_unknown_fields)]` refuses \
         every document the struct writes, and no schema is true of both. Drop \
         `deny_unknown_fields`, or drop the tag and declare it as a field",
    ))
}

/// A named field serde writes or reads under its struct's own tag is refused.
///
/// serde checks an enum's variant fields against its tag and skips a struct's,
/// so it writes the key twice, once as the tag and once as the field, and reads
/// the tag's value back as the field. The schema would require the name twice
/// and hold it to the tag's `const`. A field serde skips in the direction it
/// would collide in is no conflict there. A flattened field's own name is never
/// written, so it is exempt; the keys its type writes are not checked, since
/// they are not visible at expansion time, the limit serde's own check of an
/// enum's internal tag has too.
pub(super) fn reject_field_named_as_tag(input: &DeriveInput) -> syn::Result<()> {
    let container = Container::read(input);
    let (Some(tag), Data::Struct(data)) = (struct_tag(input, &container), &input.data) else {
        return Ok(());
    };

    let conflicting = data.fields.iter().find(|field| {
        if is_flattened(field) || is_skipped_both_ways(&field.attrs) {
            return false;
        }
        let written = !serde_flag(&field.attrs, &["skip_serializing"])
            && field_name(field, &container) == tag;
        let read = !serde_flag(&field.attrs, &["skip_deserializing"])
            && aliases::read_names(field, &container)
                .iter()
                .any(|name| name == tag);
        written || read
    });

    match conflicting.and_then(|field| field.ident.as_ref()) {
        Some(ident) => Err(syn::Error::new(
            ident.span(),
            format!(
                "`{tag}` is also this struct's `#[serde(tag = \"...\")]`, so serde writes the key \
                 twice, once as the tag and once as this field, and reads the tag's value back as \
                 the field. Rename the field or the tag, or `#[serde(skip)]` the field"
            ),
        )),
        None => Ok(()),
    }
}

/// A named field serde writes and never reads is refused in an object
/// `#[serde(deny_unknown_fields)]` closes.
///
/// `skip_deserializing` alone keeps the field out of the object serde reads,
/// which is all [`is_described`](crate::derive::schema::attributes::is_described)
/// names, and an object constraining no member it does not name still admits
/// what serde writes of it. A closed object
/// ([`closed`](crate::derive::schema::closed)) constrains every such member, so
/// it refuses what serde writes of the field. Checked in every object serde
/// writes, a struct and each struct variant it writes. A
/// `#[serde(transparent)]` struct is its one field's value, with no object to
/// check.
///
/// An open flattened field constrains such members only when its type hoists an
/// `additionalProperties`, which is its type's answer rather than its syntax's,
/// so that case is a bound
/// ([`open_fields_beside_unread_fields`](crate::derive::schema::open_fields_beside_unread_fields))
/// rather than a refusal here.
pub(super) fn reject_unread_field_in_closed_object(input: &DeriveInput) -> syn::Result<()> {
    let container = Container::read(input);
    if container.transparent || !container.deny_unknown_fields {
        return Ok(());
    }

    for fields in written_groups(input) {
        if let Some(span) = unread_field_span(fields) {
            return Err(syn::Error::new(
                span,
                "`skip_deserializing` leaves this field out of the schema, since serde never \
                 reads it, but serde still writes it, and the `additionalProperties` or \
                 `unevaluatedProperties` of `false` that `#[serde(deny_unknown_fields)]` gives \
                 this object refuses a member the schema does not name. Use `#[serde(skip)]` to \
                 leave it out both ways, or drop `skip_deserializing` so the schema names it",
            ));
        }
    }
    Ok(())
}
