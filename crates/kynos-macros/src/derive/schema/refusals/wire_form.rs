//! Forms whose wire value is not the one the declaration predicts: a
//! conversion, a function override, an untagged or catch-all decoding rule, and
//! a transparent struct that writes and reads through different fields.

use syn::{Data, DeriveInput, Field, Fields, spanned::Spanned};

use crate::derive::{
    common::skip_value,
    schema::{
        Container,
        attributes::{is_skipped_both_ways, serde_flag, serde_key_span, transparent_picks},
        described_variants, is_written,
    },
};

/// serde's keys that hand a value to a function instead of its own
/// `Serialize` and `Deserialize`.
const WIRE_FORM_OVERRIDES: &[&str] = &["with", "serialize_with", "deserialize_with"];

/// The overrides among those that serde reads through.
const READ_OVERRIDES: &[&str] = &["with", "deserialize_with"];

/// The overrides among those that serde writes through.
const WRITE_OVERRIDES: &[&str] = &["with", "serialize_with"];

/// serde's container keys that write or read the whole value as another type.
const CONVERSIONS: &[&str] = &["into", "from", "try_from"];

/// A type serde writes or reads as another type has no schema its declaration
/// predicts. Runs first, since every other rule reads the declaration. `remote`
/// is allowed: its fields mirror the type it names.
pub(super) fn reject_container_conversions(input: &DeriveInput) -> syn::Result<()> {
    let Some((key, span)) = serde_key_span(&input.attrs, CONVERSIONS) else {
        return Ok(());
    };
    let (noun, members) = match &input.data {
        Data::Enum(_) => ("enum", "variants"),
        // A union was refused at the top of `expand_inner`.
        Data::Struct(_) | Data::Union(_) => ("struct", "fields"),
    };
    Err(syn::Error::new(
        span,
        format!(
            "`{key}` makes serde read or write this {noun} as the type it names rather than as \
             the {members} it declares, so a schema derived from the declaration would describe \
             a value the wire never carries. Implement `Schema` for this {noun} by hand, \
             describing the type serde converts through -- `registry.resolve::<T>()` where that \
             is one type `T` in both directions"
        ),
    ))
}

/// `#[serde(untagged)]` has no describable decoding rule.
///
/// serde's first-match tie-break is not expressible in JSON Schema; the same
/// holds for a single untagged variant.
pub(super) fn reject_untagged(input: &DeriveInput) -> syn::Result<()> {
    // Only an enum can be untagged; serde refuses it elsewhere itself.
    let Data::Enum(data) = &input.data else {
        return Ok(());
    };

    for attr in &input.attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let mut found = None;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("untagged") {
                found = Some(meta.path.span());
            } else {
                skip_value(&meta)?;
            }
            Ok(())
        });
        if let Some(span) = found {
            return Err(syn::Error::new(
                span,
                "an untagged enum has no describable decoding rule: `anyOf` without a \
                 discriminator is ambiguous, and serde's first-match tie-break cannot be \
                 expressed. Use `#[serde(tag = \"...\")]`, which becomes a `discriminator`",
            ));
        }
    }

    for variant in described_variants(data) {
        if let Some((_, span)) = serde_key_span(&variant.attrs, &["untagged"]) {
            return Err(syn::Error::new(
                span,
                "an untagged variant has no describable decoding rule: serde writes it as its \
                 bare payload and reads it only once every tagged variant has failed, a \
                 first-match tie-break a `oneOf` cannot express. Tag the variant like its \
                 siblings, or publish the value as `Unchecked` on purpose",
            ));
        }
    }
    Ok(())
}

/// A value serde reads or writes through a function has no schema the type
/// predicts.
///
/// Scans every member serde reads (including a flattened `PhantomData`, whose
/// function still writes members), only [`READ_OVERRIDES`] where serde never
/// writes, and a transparent struct's [`transparent_picks`] per direction. A
/// newtype struct's member is always scanned, as serde writes it regardless.
pub(super) fn reject_wire_form_overrides(input: &DeriveInput) -> syn::Result<()> {
    type Scanned<'a> = (&'a [syn::Attribute], &'static str, &'static [&'static str]);

    fn fields<'a>(
        fields: &'a Fields,
        newtype: bool,
        keys: &'static [&'static str],
    ) -> Vec<Scanned<'a>> {
        let scanned: fn(&&Field) -> bool = match fields {
            Fields::Named(_) => |field| !serde_flag(&field.attrs, &["skip", "skip_deserializing"]),
            Fields::Unnamed(_) if newtype => |_| true,
            Fields::Unnamed(_) | Fields::Unit => |field| !is_skipped_both_ways(&field.attrs),
        };
        fields
            .iter()
            .filter(scanned)
            .map(|field| {
                // A scanned named field is read, so `skip_serializing` on one
                // is `skip_serializing` alone.
                let unwritten =
                    field.ident.is_some() && serde_flag(&field.attrs, &["skip_serializing"]);
                let keys = if unwritten { READ_OVERRIDES } else { keys };
                (field.attrs.as_slice(), "field", keys)
            })
            .collect()
    }

    fn picked(fields: &Fields) -> Vec<Scanned<'_>> {
        fn scanned<'a>(field: &'a Field, keys: &'static [&'static str]) -> Scanned<'a> {
            (field.attrs.as_slice(), "field", keys)
        }
        match transparent_picks(fields) {
            (Some(written), Some(read)) if std::ptr::eq(written, read) => {
                vec![scanned(written, WIRE_FORM_OVERRIDES)]
            }
            (written, read) => written
                .map(|field| scanned(field, WRITE_OVERRIDES))
                .into_iter()
                .chain(read.map(|field| scanned(field, READ_OVERRIDES)))
                .collect(),
        }
    }

    let described = match &input.data {
        Data::Struct(data) if Container::read(input).transparent => picked(&data.fields),
        Data::Struct(data) => fields(&data.fields, data.fields.len() == 1, WIRE_FORM_OVERRIDES),
        Data::Enum(data) => described_variants(data)
            .into_iter()
            .flat_map(|variant| {
                let keys = if is_written(variant) {
                    WIRE_FORM_OVERRIDES
                } else {
                    READ_OVERRIDES
                };
                let members = fields(&variant.fields, false, keys);
                std::iter::once((variant.attrs.as_slice(), "variant", keys)).chain(members)
            })
            .collect(),
        // Refused at the top of `expand_inner`.
        Data::Union(_) => Vec::new(),
    };

    for (attrs, noun, keys) in described {
        if let Some((key, span)) = serde_key_span(attrs, keys) {
            return Err(syn::Error::new(
                span,
                format!(
                    "`{key}` reads or writes this {noun} in a form its Rust type does not \
                     predict, so a schema derived from the type would describe a value the wire \
                     never carries. Give the value a newtype whose own `Serialize` and \
                     `Deserialize` produce that form and whose own `Schema` describes it"
                ),
            ));
        }
    }
    Ok(())
}

/// `#[serde(other)]` makes an enum accept every tag it does not name.
///
/// Only OpenAPI 3.2's `discriminator.defaultMapping` can say where unnamed tags
/// go, and this derive emits none, so every build refuses it.
pub(super) fn reject_catch_all(input: &DeriveInput) -> syn::Result<()> {
    let Data::Enum(data) = &input.data else {
        return Ok(());
    };

    for variant in described_variants(data) {
        if let Some((_, span)) = serde_key_span(&variant.attrs, &["other"]) {
            return Err(syn::Error::new(
                span,
                "`#[serde(other)]` accepts every tag this enum does not name, and only OpenAPI \
                 3.2's `discriminator.defaultMapping` can say where those go, which this derive \
                 does not emit. Name every variant the API accepts, or publish the value as \
                 `Unchecked` on purpose",
            ));
        }
    }
    Ok(())
}

/// A `#[serde(transparent)]` struct serde writes through one field and reads
/// through another is refused.
///
/// Fields are compared, not schemas ([`transparent_picks`]). Where a direction
/// picks no single field, serde itself refuses that direction's derive.
pub(super) fn reject_transparent_without_one_field(input: &DeriveInput) -> syn::Result<()> {
    let Data::Struct(data) = &input.data else {
        return Ok(());
    };
    let Some((_, span)) = serde_key_span(&input.attrs, &["transparent"]) else {
        return Ok(());
    };
    let (Some(written), Some(read)) = transparent_picks(&data.fields) else {
        return Ok(());
    };
    if std::ptr::eq(written, read) {
        return Ok(());
    }

    let label = |member: &Field| {
        member.ident.as_ref().map_or_else(
            || {
                let index = data
                    .fields
                    .iter()
                    .position(|field| std::ptr::eq(field, member))
                    .unwrap_or_default();
                format!("field {index}")
            },
            |ident| format!("`{ident}`"),
        )
    };
    let (writes, reads) = (label(written), label(read));

    Err(syn::Error::new(
        span,
        format!(
            "`#[serde(transparent)]` makes serde write through the one field without `skip` or \
             `skip_serializing` and read through the one field without `skip`, \
             `skip_deserializing` or `default`, and `Schema` describes the struct only where \
             they are the same field, since it does not compare two fields' schemas; this struct \
             writes through {writes} and reads through {reads}. Leave \
             one field serde both writes and reads, and mark every other `#[serde(skip)]`"
        ),
    ))
}
