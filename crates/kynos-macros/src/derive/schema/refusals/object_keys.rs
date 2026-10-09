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
/// flattened open map: serde refuses unnamed keys before the map sees them, so
/// it reads empty while serde writes its members.
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
        // An unflattened `open` is `check_constraints`' diagnostic.
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

/// The `#[serde(tag = "...")]` serde writes for a named, non-transparent struct.
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
/// The closed struct refuses the tag it writes.
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
/// serde does not check this for a struct, so it writes the key twice. A
/// flattened field's keys are invisible here, as they are to serde's own check.
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
/// The schema omits the field, and the closed object then refuses what serde
/// writes of it. The open-map case is a bound instead
/// ([`open_fields_beside_unread_fields`](crate::derive::schema::open_fields_beside_unread_fields)).
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
