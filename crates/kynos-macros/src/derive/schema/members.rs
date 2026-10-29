//! Which members serde puts on the wire, grouped as the schema describes them,
//! and the names other derives take from them.

use proc_macro2::Span;
use syn::{
    Data, DataEnum, DeriveInput, Field, Fields, FieldsNamed, Variant, punctuated::Punctuated,
    spanned::Spanned, token::Comma,
};

use super::{
    attributes::{
        field_name, field_read_name, is_skipped_both_ways, serde_flag, serde_key_span, sides,
    },
    container::{Container, Visitor},
};
use crate::derive::common::skip_value;

/// The field groups of every object serde writes: a struct's, and each
/// variant's serde writes.
pub(super) fn written_groups(input: &DeriveInput) -> Vec<&Fields> {
    match &input.data {
        Data::Struct(data) => vec![&data.fields],
        Data::Enum(data) => data
            .variants
            .iter()
            .filter(|variant| is_written(variant))
            .map(|variant| &variant.fields)
            .collect(),
        // Refused at the top of `expand_inner`.
        Data::Union(_) => Vec::new(),
    }
}

/// Each group of fields the input declares: a struct's, or one per variant.
pub(super) fn field_groups(input: &DeriveInput) -> Vec<&Fields> {
    match &input.data {
        Data::Struct(data) => vec![&data.fields],
        Data::Enum(data) => data
            .variants
            .iter()
            .map(|variant| &variant.fields)
            .collect(),
        Data::Union(_) => Vec::new(),
    }
}

/// The field groups the emitted schema describes: every group but that of a
/// variant serde skips both ways.
///
/// Attribute grammar still reads [`field_groups`]: a malformed attribute is an
/// error wherever it is written.
pub(super) fn described_groups<'a>(
    input: &'a DeriveInput,
    container: &Container,
) -> Vec<&'a Fields> {
    match &input.data {
        Data::Enum(data) => described_variants(data, container)
            .into_iter()
            .map(|variant| &variant.fields)
            .collect(),
        _ => field_groups(input),
    }
}

/// The variants the emitted schema describes: every one serde does not skip
/// both ways. A read-only variant is among them, since serde accepts it.
///
/// An identifier enum's are all of them: serde's identifier visitor names
/// every declared variant and reads no `skip`, and serde refuses to write one.
pub(super) fn described_variants<'a>(
    data: &'a DataEnum,
    container: &Container,
) -> Vec<&'a Variant> {
    data.variants
        .iter()
        .filter(|variant| {
            container.visitor == Visitor::Identifier || !is_skipped_both_ways(&variant.attrs)
        })
        .collect()
}

/// Whether serde writes a variant: it carries neither `skip` nor
/// `skip_serializing`.
pub(super) fn is_written(variant: &Variant) -> bool {
    !serde_flag(&variant.attrs, &["skip", "skip_serializing"])
}

/// Where the first named field serde writes and never reads, `skip_deserializing`
/// alone, carries that key.
pub(super) fn unread_field_span(fields: &Fields) -> Option<Span> {
    let Fields::Named(named) = fields else {
        return None;
    };
    named
        .named
        .iter()
        .find_map(|field| one_way_skip_span(field, &["skip_deserializing"]))
        .map(|(_, span)| span)
}

/// The first of `keys` a member carries without the other half of
/// `#[serde(skip)]`, and where it is written.
pub(super) fn one_way_skip_span(field: &Field, keys: &[&str]) -> Option<(String, Span)> {
    if is_skipped_both_ways(&field.attrs) {
        return None;
    }
    serde_key_span(&field.attrs, keys)
}

/// The members of a tuple or tuple variant that hold a position on the wire,
/// in order: each one serde does not skip both ways. Not
/// [`is_described`](super::attributes::is_described), which would drop a
/// `skip_deserializing` member the refusals must still see.
pub(super) fn positional_members(fields: &Punctuated<Field, Comma>) -> Vec<&Field> {
    fields
        .iter()
        .filter(|field| !is_skipped_both_ways(&field.attrs))
        .collect()
}

/// The fewest elements serde reads for a tuple holding these positions.
///
/// Every position up to the last one with no `#[serde(default)]`, since serde
/// fills only trailing defaults. Under a container default (`defaulted`) it
/// fills every missing element, so there is no bound.
pub(super) fn min_items(positions: &[&Field], defaulted: bool) -> u64 {
    if defaulted {
        return 0;
    }
    let required = positions
        .iter()
        .rposition(|field| !serde_flag(&field.attrs, &["default"]))
        .map_or(0, |last| last + 1);
    u64::try_from(required).unwrap_or(u64::MAX)
}

/// The names this derive would describe a struct's fields under, in order.
/// Shared with [`multipart`](crate::derive::multipart) so part and property
/// names agree, and with [`common::wire_names`](crate::derive::common::wire_names)
/// so a parameter's name falls back to the property name.
pub(crate) fn property_names(input: &DeriveInput, fields: &FieldsNamed) -> Vec<String> {
    let container = Container::read(input);
    fields
        .named
        .iter()
        .map(|field| field_name(field, &container))
        .collect()
}

/// The span of the first field whose split `rename` gives serde's two
/// directions different names, at that `rename`;
/// [`multipart`](crate::derive::multipart) refuses it, since a part has one
/// name.
pub(crate) fn split_renamed_field(input: &DeriveInput, fields: &FieldsNamed) -> Option<Span> {
    let container = Container::read(input);
    fields
        .named
        .iter()
        .find(|field| field_name(field, &container) != field_read_name(field, &container))
        .map(|field| {
            serde_key_span(&field.attrs, &["rename"]).map_or_else(|| field.span(), |(_, span)| span)
        })
}

/// The span of a split `key(serialize = ..., deserialize = ...)` in `attrs`
/// whose sides differ, one side left out included, at that `key`. Shape errors
/// stay serde's to report.
pub(crate) fn split_rule(attrs: &[syn::Attribute], key: &str) -> Option<Span> {
    let mut found = None;
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let _ = attr.parse_nested_meta(|meta| {
            if !meta.path.is_ident(key) {
                return skip_value(&meta);
            }
            let sides = sides(&meta)?;
            if found.is_none() && sides.serialize != sides.deserialize {
                found = Some(meta.path.span());
            }
            Ok(())
        });
    }
    found
}
