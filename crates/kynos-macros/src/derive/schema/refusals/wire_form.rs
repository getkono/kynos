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
/// predicts.
///
/// `into`, `from` and `try_from` hand the whole value to a conversion, so the
/// wire carries whatever the named type writes, and the fields or variants
/// declared here reach it only through code the derive cannot read. Refused on
/// a struct and an enum alike, which is everywhere serde accepts the keys, and
/// before any other rule, since every other rule reads a declaration this one
/// says the wire does not follow. `remote` is not among them: its fields mirror
/// the type it names, so the declaration still predicts the wire form.
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
/// `anyOf` with no discriminator leaves a consumer to guess which branch a
/// payload is, and serde's first-match tie-break is not expressible in JSON
/// Schema. An internally or adjacently tagged enum becomes a `discriminator`,
/// which is.
///
/// The same holds for one variant marked untagged: serde writes it as its bare
/// payload and reads it only once every tagged variant has failed, so it is
/// refused on every variant the schema describes rather than emitted as a
/// tagged branch the wire never carries. A variant serde skips both ways is in
/// no schema and is left alone.
pub(super) fn reject_untagged(input: &DeriveInput) -> syn::Result<()> {
    // Only an enum can be untagged. serde refuses the attribute anywhere else
    // in its own words, and a second diagnostic calling a struct an enum is
    // this derive restating a serde shape rule and misnaming the shape.
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
/// Refused on every field and variant the schema describes, which is
/// everywhere serde accepts the three keys, and on a flattened `PhantomData`
/// serde reads: that marker is in no schema, but serde hands the function its
/// flattening serializer and deserializer, which write and demand whatever
/// members the function names. Any other `PhantomData` is described as `null`,
/// which an override contradicts as it would any other type's schema. A named
/// field serde never reads and every field of a variant serde skips both ways
/// are in no schema, so an override on one of them contradicts nothing and is
/// left alone.
///
/// Where serde never writes -- inside a variant serde never writes, and on a
/// named field carrying `skip_serializing` alone -- only [`READ_OVERRIDES`] are
/// refused: [`is_written`] says why `serialize_with` changes nothing there.
///
/// A `#[serde(transparent)]` struct is scanned only on the one field each
/// direction picks, from [`transparent_picks`]: a field picked both ways for
/// [`WIRE_FORM_OVERRIDES`], one picked for writing alone for
/// [`WRITE_OVERRIDES`], one picked for reading alone for [`READ_OVERRIDES`].
/// serde derives no direction with several candidates, and applies an override
/// to the picked field alone, so no other field's value reaches a function in
/// either direction.
///
/// An unnamed member is exempt only when serde skips it both ways, and never on
/// a newtype struct, whose member serde writes through the function whatever it
/// skips. Skip attributes are read rather than
/// [`is_described`](crate::derive::schema::attributes::is_described), which
/// would exempt that newtype member and a flattened `PhantomData` serde reads.
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
/// The schema's `oneOf` lists only the named ones, and only OpenAPI 3.2's
/// `discriminator.defaultMapping` can say where the rest go. This derive emits
/// no `defaultMapping`, so every build refuses the attribute rather than 3.1
/// alone. Only a variant serde reads is checked: `skip_serializing` keeps a
/// catch-all out of what serde writes, not out of deserialization, but serde
/// draws the fallthrough only from the variants it reads, so `other` on one it
/// skips both ways catches nothing. A lone `skip_deserializing` is refused
/// before this runs, by
/// [`reject_unread_variant`](super::skips::reject_unread_variant).
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
/// serde picks per direction, from the attributes alone ([`transparent_picks`]):
/// it writes through the field without `skip` or `skip_serializing`, reads
/// through the field without `skip`, `skip_deserializing` or a field-level
/// `default`, and never through a `PhantomData`. Where each direction picks a
/// single field and they are different fields, the struct is refused: the
/// derive compares fields rather than their schemas, so two fields of one type
/// are refused too. Everything else is
/// [`transparent_member`](crate::derive::schema::attributes::transparent_member)'s,
/// which `struct_body` describes. Where only one direction picks a single field,
/// serde refuses the other derive by itself, so the struct compiles with that
/// direction's derive alone and the field is true of it. Where neither does,
/// serde refuses the struct for either derive, and a second error here would
/// restate it; that covers a unit struct too, and an enum is serde's to refuse.
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
