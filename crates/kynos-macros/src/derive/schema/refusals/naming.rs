//! Members serde writes under one name and reads under another, so no name a
//! schema gives them is true in both directions.

use syn::{Data, DeriveInput, Fields, Variant};

use crate::derive::schema::{
    aliases,
    aliases::shadowed_variant,
    attributes::{
        field_name, field_read_name, is_flattened, rename_all_fields, serde_flag, serde_key_span,
        variant_name, variant_read_name, variant_rename_all,
    },
    container::{Container, Visitor},
    members::{described_variants, is_written, split_rule},
};

/// A variant whose own name serde reads as an earlier variant has no schema
/// true of both directions: serde writes it under a name it reads back as the
/// earlier one. A merely shadowed alias is left out by
/// [`aliases::variants_read_names`] instead.
///
/// An identifier enum is never written, so the shared name only reads as the
/// earlier variant and `variants_read_names` lists it once.
pub(super) fn reject_shadowed_variant(
    input: &DeriveInput,
    container: &Container,
) -> syn::Result<()> {
    let Data::Enum(data) = &input.data else {
        return Ok(());
    };

    if container.visitor == Visitor::Identifier {
        return Ok(());
    }
    let Some((later, earlier)) = shadowed_variant(&described_variants(data, container), container)
    else {
        return Ok(());
    };
    Err(syn::Error::new(
        later.ident.span(),
        format!(
            "serde reads `{name}`, this variant's own name, as `{earlier}`, the earlier variant \
             that also claims it, so `{later}` goes on the wire under a name that reads back as \
             `{earlier}`, and no schema describing `{later}` is true in both directions. Drop \
             the `rename` or `alias` that gives both variants the name",
            name = variant_read_name(later, container),
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
/// flattened and transparent fields, whose own names are not on the wire. An
/// identifier enum's variants are all read-only: serde never writes the enum,
/// so [`variant_name`] gives each the name it is read under.
pub(super) fn reject_split_rename(input: &DeriveInput, container: &Container) -> syn::Result<()> {
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
            let variants = described_variants(data, container);
            let read_names = aliases::variants_read_names(&variants, container);
            variants
                .into_iter()
                .zip(read_names)
                .filter(|(variant, _)| is_written(variant))
                .find_map(|(variant, read_names)| {
                    let written = variant_name(variant, container);
                    let read = variant_read_name(variant, container);
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

/// A split case rule whose sides differ, one side left out included: a
/// container `rename_all(serialize = ..., deserialize = ...)`, or, for a struct
/// variant serde writes whose two sides differ once each side its own
/// `rename_all` leaves takes the enum's `rename_all_fields`, as serde fills it,
/// that `rename_all_fields(...)` where the variant leaves it a side, else the
/// variant's own `rename_all(...)`.
///
/// Such a rule gives a member two wire names. A container rule is refused
/// whichever members it reaches, save on an identifier enum, which serde never
/// writes and [`Container`] reads by its deserialize side. A field rule is
/// refused only where it reaches a struct variant serde writes: one reaching
/// only variants serde skips or only reads, unit or tuple variants, or sides
/// that resolve to one style is not refused.
/// Runs before any check that reads a [`Container`]'s `rename_all`; the
/// refusals ahead of it read no rename rule.
pub(super) fn reject_split_rename_all(
    input: &DeriveInput,
    container: &Container,
) -> syn::Result<()> {
    let identifier = container.visitor == Visitor::Identifier;
    let variants = match &input.data {
        Data::Enum(data) => data.variants.iter().collect(),
        _ => Vec::new(),
    };
    let fields_rule = rename_all_fields(input);
    // Each struct variant serde writes whose sides differ once filled, and the
    // rules that give it them: `rename_all_fields` only where the variant's
    // own rule leaves it a side.
    let variant_rules = variants
        .into_iter()
        .filter(|variant| matches!(variant.fields, Fields::Named(_)) && is_written(variant))
        .filter_map(|variant: &Variant| {
            let own = variant_rename_all(variant);
            let leaves_a_side = own.serialize.is_none() || own.deserialize.is_none();
            let resolves_split = own.serialize.or_else(|| fields_rule.serialize.clone())
                != own.deserialize.or_else(|| fields_rule.deserialize.clone());
            resolves_split.then_some([
                leaves_a_side.then_some((
                    &input.attrs,
                    "rename_all_fields",
                    "the fields of a variant it reaches",
                )),
                Some((&variant.attrs, "rename_all", "every field of this variant")),
            ])
        })
        .flatten()
        .flatten();
    let container_rule = (!identifier).then_some((&input.attrs, "rename_all", "every member"));
    let rules = container_rule.into_iter().chain(variant_rules);
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
