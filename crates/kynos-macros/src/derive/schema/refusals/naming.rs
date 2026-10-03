//! Members serde writes under one name and reads under another, so no name a
//! schema gives them is true in both directions.

use syn::{Data, DeriveInput, Fields};

use crate::derive::schema::{
    Container, aliases,
    aliases::shadowed_variant,
    attributes::{
        field_name, field_read_name, is_flattened, serde_flag, serde_key_span, variant_name,
        variant_read_name, variant_rename_all,
    },
    described_variants, is_written, split_rule,
};

/// A variant whose own name serde reads as an earlier variant has no schema
/// true of both directions.
///
/// serde reads a name as the first variant claiming it, by its wire name or an
/// `alias`, and does not refuse the collision, so it writes the later variant
/// under a name it reads back as the earlier one: naming the later variant's
/// branch or `enum` item there describes a request serde reads otherwise, and
/// leaving the name out describes a response serde writes. A variant serde
/// reads and never writes is refused alike, since its own name is dead and its
/// aliases could all be too. An alias an earlier variant claims is only
/// unreachable, and [`aliases::variants_read_names`] leaves it out. A variant
/// serde skips both ways claims nothing.
pub(super) fn reject_shadowed_variant(input: &DeriveInput) -> syn::Result<()> {
    let Data::Enum(data) = &input.data else {
        return Ok(());
    };

    let container = Container::read(input);
    let Some((later, earlier)) = shadowed_variant(&described_variants(data), &container) else {
        return Ok(());
    };
    Err(syn::Error::new(
        later.ident.span(),
        format!(
            "serde reads `{name}`, this variant's own name, as `{earlier}`, the earlier variant \
             that also claims it, so `{later}` goes on the wire under a name that reads back as \
             `{earlier}`, and no schema describing `{later}` is true in both directions. Drop \
             the `rename` or `alias` that gives both variants the name",
            name = variant_read_name(later, &container),
            earlier = earlier.ident,
            later = later.ident,
        ),
    ))
}

/// A member serde both writes and reads is refused where a split `rename`
/// gives the two directions different names and serde never reads the written
/// one.
///
/// One schema serves both directions, so no name it gives the member is true of
/// both: under the written name it describes a request serde refuses, under the
/// read name a response serde never writes. Where serde also reads the written
/// name, through an `alias`, the member is described under every name serde
/// reads it as, the written one among them, which is true both ways; a variant's
/// alias an earlier variant claims is not read as it, so it does not count. A
/// member serde uses one way is
/// named by that side, so it is exempt: a field serde skips in either
/// direction, every field of a variant serde never writes, and a variant serde
/// only reads. A flattened field's own name is neither written nor read, and
/// neither is a transparent struct's field's. A variant serde only writes is
/// refused before this runs, by
/// [`reject_unread_variant`](super::skips::reject_unread_variant).
pub(super) fn reject_split_rename(input: &DeriveInput) -> syn::Result<()> {
    let container = Container::read(input);
    // Each group of fields serde writes, under the container naming them.
    let groups = match &input.data {
        Data::Struct(data) if !container.transparent => vec![(&data.fields, container.clone())],
        Data::Enum(data) => data
            .variants
            .iter()
            .filter(|variant| is_written(variant))
            .map(|variant| (&variant.fields, container.fields_of(variant)))
            .collect(),
        _ => Vec::new(),
    };
    let field = groups
        .iter()
        .flat_map(|(fields, naming)| fields.iter().map(move |field| (field, naming)))
        .filter(|(field, _)| {
            !is_flattened(field)
                && !serde_flag(
                    &field.attrs,
                    &["skip", "skip_serializing", "skip_deserializing"],
                )
        })
        .find_map(|(field, naming)| {
            let ident = field.ident.as_ref()?;
            let written = field_name(field, naming);
            let read = field_read_name(field, naming);
            let unread = !aliases::read_names(field, naming).contains(&written);
            unread.then(|| {
                let skip = "`skip_serializing` or `skip_deserializing` the field";
                ("field", skip, &field.attrs, ident.span(), written, read)
            })
        });
    let variant = match &input.data {
        Data::Enum(data) => {
            let variants = described_variants(data);
            let read_names = aliases::variants_read_names(&variants, &container);
            variants
                .into_iter()
                .zip(read_names)
                .filter(|(variant, _)| is_written(variant))
                .find_map(|(variant, read_names)| {
                    let written = variant_name(variant, &container);
                    let read = variant_read_name(variant, &container);
                    // An unsplit name an earlier variant claims is
                    // `reject_shadowed_variant`'s to refuse.
                    let unread = written != read && !read_names.contains(&written);
                    unread.then(|| {
                        let skip = "`skip_serializing` the variant";
                        (
                            "variant",
                            skip,
                            &variant.attrs,
                            variant.ident.span(),
                            written,
                            read,
                        )
                    })
                })
        }
        _ => None,
    };

    let Some((member, skip, attrs, ident, written, read)) = field.or(variant) else {
        return Ok(());
    };
    let span = serde_key_span(attrs, &["rename"]).map_or(ident, |(_, span)| span);
    Err(syn::Error::new(
        span,
        format!(
            "serde writes this {member} as `{written}` and reads it as `{read}`, never as \
             `{written}`, so no schema naming it is true in both directions. Give both sides \
             one name with `rename = \"...\"`, or {skip}"
        ),
    ))
}

/// A split case rule whose sides differ, one side left out included, where it
/// names a member serde both writes and reads: a container
/// `rename_all(serialize = ..., deserialize = ...)`, an enum's
/// `rename_all_fields(...)` that reaches a struct variant, or the own
/// `rename_all(...)` of a struct variant serde writes.
///
/// Such a rule gives a member two wire names, and one schema describes both
/// directions, so the form is refused as the parameter derives refuse a split
/// `rename_all`. A variant serde writes is one it also reads, since a lone
/// `skip_deserializing` is refused before this runs, by
/// [`reject_unread_variant`](super::skips::reject_unread_variant), so the
/// variant filter is [`is_written`], as in [`reject_split_rename`]. A rule
/// naming no field serde both writes and reads is not refused: a variant serde
/// skips both ways is in no schema, a variant serde only reads has its fields
/// named by its rule's deserialize side in [`Container::fields_of`], a unit or
/// tuple variant has no named field for its own rule to name, and a
/// `rename_all_fields` every struct variant overrides on both sides, or on an
/// enum with none, names nothing. Sides that agree are the `key = "..."` they
/// spell, and [`Container`] and [`variant_rename_all`] read them so. Runs
/// before any check that reads a [`Container`].
pub(super) fn reject_split_rename_all(input: &DeriveInput) -> syn::Result<()> {
    let variants = match &input.data {
        Data::Enum(data) => data.variants.iter().collect(),
        _ => Vec::new(),
    };
    // A struct variant whose own rule leaves a side to `rename_all_fields`.
    let fields_reached = variants.iter().any(|variant| {
        let own = variant_rename_all(variant);
        matches!(variant.fields, Fields::Named(_))
            && (own.serialize.is_none() || own.deserialize.is_none())
    });
    let rules = [
        Some((&input.attrs, "rename_all", "every member")),
        fields_reached.then_some((
            &input.attrs,
            "rename_all_fields",
            "every variant field it reaches",
        )),
    ]
    .into_iter()
    .flatten()
    .chain(
        variants
            .into_iter()
            .filter(|variant| matches!(variant.fields, Fields::Named(_)) && is_written(variant))
            .map(|variant| (&variant.attrs, "rename_all", "every field of this variant")),
    );
    for (attrs, key, reach) in rules {
        if let Some(span) = split_rule(attrs, key) {
            return Err(syn::Error::new(
                span,
                format!(
                    "a split `{key}` whose sides differ gives {reach} two wire names, and one \
                     schema describes both directions. Say which with `{key} = \"...\"`",
                ),
            ));
        }
    }
    Ok(())
}
