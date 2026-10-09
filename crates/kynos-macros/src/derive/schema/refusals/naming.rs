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
/// true of both directions: serde writes it under a name it reads back as the
/// earlier one. A merely shadowed alias is left out by
/// [`aliases::variants_read_names`] instead.
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
/// one (an `alias` of the written name makes it true both ways).
///
/// A member serde uses one way is named by that side and exempt, as are
/// flattened and transparent fields, whose own names are not on the wire.
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
/// Such a rule gives a member two wire names. A rule naming no field serde both
/// writes and reads is not refused. Runs before any check that reads a
/// [`Container`].
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
