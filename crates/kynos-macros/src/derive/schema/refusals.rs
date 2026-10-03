//! What the `Schema` derive refuses before it emits anything.
//!
//! Each rule names a form whose declaration predicts no schema true of the
//! wire in both directions, or a `#[schema(...)]` outside the grammar, and the
//! expansion reads only inputs every rule here has passed.

use proc_macro2::Span;
use syn::{
    Data, DeriveInput, Field, Fields, Lit, LitStr, punctuated::Punctuated, spanned::Spanned,
    token::Comma,
};

use super::{
    COUNTS, Container, NUMERIC, aliases,
    aliases::shadowed_variant,
    attributes::{
        described_members, field_name, field_read_name, is_described, is_flattened, is_open,
        is_option, is_required, is_skipped_both_ways, is_unit_like, open_span, serde_flag,
        serde_key_span, transparent_picks, variant_name, variant_read_name, variant_rename_all,
    },
    described_groups, described_variants, field_groups, is_written, one_way_skip_span,
    positional_members, skip_value, split_rule, unread_field_span, written_groups,
};

/// Keys written alone, with no value.
const FLAGS: &[&str] = &["unique_items", "open"];

/// serde's keys that hand a value to a function instead of its own
/// `Serialize` and `Deserialize`.
const WIRE_FORM_OVERRIDES: &[&str] = &["with", "serialize_with", "deserialize_with"];

/// The overrides among those that serde reads through.
const READ_OVERRIDES: &[&str] = &["with", "deserialize_with"];

/// The overrides among those that serde writes through.
const WRITE_OVERRIDES: &[&str] = &["with", "serialize_with"];

/// serde's container keys that write or read the whole value as another type.
const CONVERSIONS: &[&str] = &["into", "from", "try_from"];

/// Runs every refusal, in the order the later ones rely on.
///
/// Several rules say a form is refused "before this runs" by another; the
/// order here is what makes that true.
pub(super) fn check(input: &DeriveInput) -> syn::Result<()> {
    reject_container_conversions(input)?;
    reject_untagged(input)?;
    reject_unread_variant(input)?;
    reject_split_rename_all(input)?;
    reject_split_rename(input)?;
    reject_shadowed_variant(input)?;
    reject_wire_form_overrides(input)?;
    reject_catch_all(input)?;
    reject_transparent_without_one_field(input)?;
    reject_read_required_skip(input)?;
    reject_contradicted_closure(input)?;
    reject_closed_tagged_struct(input)?;
    reject_field_named_as_tag(input)?;
    reject_unread_field_in_closed_object(input)?;
    reject_one_way_member_skip(input)?;
    reject_skipped_adjacent_payload(input)?;
    check_constraints(input)
}

/// Validates every `#[schema(...)]` in the input.
///
/// Run before any code is emitted, so that
/// [`constraints`](super::attributes::constraints) can read the same lists back
/// without checking them again — a key that reached the emitter had its shape
/// settled here, and one that did not never gets there.
fn check_constraints(input: &DeriveInput) -> syn::Result<()> {
    for group in field_groups(input) {
        let named = match group {
            Fields::Named(named) => &named.named,
            Fields::Unnamed(unnamed) => &unnamed.unnamed,
            Fields::Unit => continue,
        };

        // One `unevaluatedProperties` per emitted object, so one `open` field
        // per group of fields that becomes one.
        let mut opened: Option<Span> = None;

        for field in named {
            for attr in &field.attrs {
                if attr.path().is_ident("schema") {
                    attr.parse_nested_meta(|meta| check_constraint(&meta))?;
                }
            }

            let Some(span) = open_span(field) else {
                continue;
            };

            if !is_flattened(field) {
                return Err(syn::Error::new(
                    span,
                    "`#[schema(open)]` says what a flattened field contributes to the object \
                     carrying it, and only a flattened field has anything to contribute: an \
                     ordinary field is one property, whose own schema already states what it \
                     admits. Add `#[serde(flatten)]`, or drop the attribute",
                ));
            }

            if opened.is_some() {
                return Err(syn::Error::new(
                    span,
                    "`#[schema(open)]` may appear once per container object: it supplies that \
                     object's `unevaluatedProperties`, which is one keyword, so a second open \
                     field could only overwrite what the first one said. Merge the two maps, or \
                     give one of them a named field of its own",
                ));
            }

            opened = Some(span);
        }
    }
    Ok(())
}

/// One `key` or `key = value` inside a field's `#[schema(...)]`.
fn check_constraint(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<()> {
    let Some(key) = meta.path.get_ident() else {
        return Ok(());
    };
    let name = key.to_string();

    if name == "format" {
        return Err(syn::Error::new(
            key.span(),
            "`format` says what a value *is*, which follows from its type rather than from the \
             field carrying it. Use a type that already claims the format -- `uuid::Uuid` behind \
             the `uuid` feature, a date or time type behind `time-chrono` or `time-jiff`, a \
             decimal behind `decimal-rust` or `decimal-big` -- or give the value a newtype with \
             its own `Schema` implementation. `pattern` is here if what you meant is a \
             constraint on this field rather than a claim about the type",
        ));
    }

    if FLAGS.contains(&name.as_str()) {
        // A flag: `unique_items = true` would let `= false` mean something the
        // absence of the key already means.
        return if meta.input.peek(syn::Token![=]) {
            Err(syn::Error::new(
                key.span(),
                format!("`{name}` is a flag; write it alone, or leave it out"),
            ))
        } else {
            Ok(())
        };
    }

    if NUMERIC.contains(&name.as_str()) {
        return match meta.value()?.parse()? {
            Lit::Int(_) | Lit::Float(_) => Ok(()),
            other => Err(syn::Error::new(
                other.span(),
                format!("`{name}` takes a number"),
            )),
        };
    }

    if COUNTS.contains(&name.as_str()) {
        let literal = meta.value()?.parse()?;
        return match &literal {
            Lit::Int(value) => value.base10_parse::<u64>().map(|_| ()),
            other => Err(syn::Error::new(
                other.span(),
                format!("`{name}` takes a non-negative whole number"),
            )),
        };
    }

    if name == "pattern" {
        return meta.value()?.parse::<LitStr>().map(|_| ());
    }

    Err(syn::Error::new(
        key.span(),
        format!(
            "`{name}` is not part of the `#[schema(...)]` grammar, which is the keys of \
             `kynos::schema::constraints::Constraints`: `minimum`, `maximum`, \
             `exclusive_minimum`, `exclusive_maximum`, `multiple_of`, `min_length`, \
             `max_length`, `pattern`, `min_items`, `max_items` and `unique_items`; plus \
             `open`, which says a flattened field's members are not named"
        ),
    ))
}

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
fn reject_container_conversions(input: &DeriveInput) -> syn::Result<()> {
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
fn reject_untagged(input: &DeriveInput) -> syn::Result<()> {
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
/// skips. Skip attributes are read rather than [`is_described`], which would
/// exempt that newtype member and a flattened `PhantomData` serde reads.
fn reject_wire_form_overrides(input: &DeriveInput) -> syn::Result<()> {
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

/// A variant serde writes and never reads has no closed schema true of both.
///
/// `skip_deserializing` alone keeps the variant in what serde writes and out of
/// what it reads, so a `oneOf` or `enum` listing it describes a request serde
/// refuses, and one leaving it out describes a response serde writes.
/// `skip_serializing` alone is the other way round and needs no refusal: every
/// variant serde writes is one it also reads, so the schema listing the variant
/// is true of both. A variant serde skips both ways is in no schema.
fn reject_unread_variant(input: &DeriveInput) -> syn::Result<()> {
    let Data::Enum(data) = &input.data else {
        return Ok(());
    };

    for variant in described_variants(data) {
        if let Some((_, span)) = serde_key_span(&variant.attrs, &["skip_deserializing"]) {
            return Err(syn::Error::new(
                span,
                "`skip_deserializing` makes serde write this variant and refuse to read it back, \
                 so no closed `oneOf` or `enum` is true in both directions: listing the variant \
                 describes a request serde refuses, and leaving it out describes a response \
                 serde writes. Use `#[serde(skip)]` to leave it out both ways, or drop \
                 `skip_deserializing`",
            ));
        }
    }
    Ok(())
}

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
fn reject_shadowed_variant(input: &DeriveInput) -> syn::Result<()> {
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
/// refused before this runs, by [`reject_unread_variant`].
fn reject_split_rename(input: &DeriveInput) -> syn::Result<()> {
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
/// [`reject_unread_variant`], so the variant filter is [`is_written`], as in
/// [`reject_split_rename`]. A rule naming no field serde both writes and reads
/// is not refused: a variant serde skips both ways is in no schema, a variant
/// serde only reads has its fields named by its rule's deserialize side in
/// [`Container::fields_of`], a unit or tuple variant has no named field for its
/// own rule to name, and a `rename_all_fields` every struct variant overrides
/// on both sides, or on an enum with none, names nothing. Sides that agree are the `key = "..."` they
/// spell, and [`Container`] and [`variant_rename_all`] read them so. Runs
/// before any check that reads a [`Container`].
fn reject_split_rename_all(input: &DeriveInput) -> syn::Result<()> {
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

/// `#[serde(other)]` makes an enum accept every tag it does not name.
///
/// The schema's `oneOf` lists only the named ones, and only OpenAPI 3.2's
/// `discriminator.defaultMapping` can say where the rest go. This derive emits
/// no `defaultMapping`, so every build refuses the attribute rather than 3.1
/// alone. Only a variant serde reads is checked: `skip_serializing` keeps a
/// catch-all out of what serde writes, not out of deserialization, but serde
/// draws the fallthrough only from the variants it reads, so `other` on one it
/// skips both ways catches nothing. A lone `skip_deserializing` is refused
/// before this runs, by [`reject_unread_variant`].
fn reject_catch_all(input: &DeriveInput) -> syn::Result<()> {
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
/// [`transparent_member`](super::attributes::transparent_member)'s, which
/// `struct_body` describes. Where only one direction picks a single field, serde
/// refuses the other derive by itself, so the struct compiles with that
/// direction's derive alone and the field is true of it. Where neither does,
/// serde refuses the struct for either derive, and a second error here would
/// restate it; that covers a unit struct too, and an enum is serde's to refuse.
fn reject_transparent_without_one_field(input: &DeriveInput) -> syn::Result<()> {
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

/// `skip_serializing_if` on a field serde still requires on read has no
/// truthful `required`, and neither has `skip_serializing` alone, which is
/// `skip_serializing_if` with a condition that always holds.
///
/// serde may leave such a field out of what it writes, and rejects a document
/// without it on read, so listing it in `required` misdescribes a response and
/// leaving it out misdescribes a request. An `Option`, a field-level
/// `#[serde(default)]` or a struct's container `#[serde(default)]` lets the
/// field be absent both ways, which is what lets [`is_required`] leave it out;
/// this refusal reads that same rule, so the two cannot disagree. A flattened
/// field is decided before that rule, by `#[schema(open)]` alone, because serde
/// ignores any default on it. Only named fields are checked, since only an
/// object has a `required` list; a field serde never reads is in no schema, and a
/// field of a variant serde never writes is only read, where neither key changes
/// anything. A `#[serde(transparent)]` struct is not checked at all: serde writes
/// its one field's value whatever `skip_serializing_if` says, and the schema
/// describing that value has no `required` list to contradict.
fn reject_read_required_skip(input: &DeriveInput) -> syn::Result<()> {
    let container = Container::read(input);
    if container.transparent {
        return Ok(());
    }

    let groups: Vec<&Fields> = match &input.data {
        Data::Struct(data) => vec![&data.fields],
        Data::Enum(data) => data
            .variants
            .iter()
            .filter(|variant| is_written(variant))
            .map(|variant| &variant.fields)
            .collect(),
        // Refused at the top of `expand_inner`.
        Data::Union(_) => Vec::new(),
    };

    let named = groups.into_iter().filter_map(|fields| match fields {
        Fields::Named(named) => Some(&named.named),
        Fields::Unnamed(_) | Fields::Unit => None,
    });

    for field in named.flatten().filter(|field| is_described(field)) {
        let Some((key, span)) =
            serde_key_span(&field.attrs, &["skip_serializing_if", "skip_serializing"])
        else {
            continue;
        };

        // A flattened field is decided by `#[schema(open)]` alone, before any
        // default: serde ignores `#[serde(default)]` on a flattened field, at
        // field and container level alike. An open map reads absent as empty
        // and is never listed in `required`, recognised by the same pair
        // `object_body` reads. The field's Rust type is invisible here, and
        // this error aborts expansion before any `Flatten` or `OpenMap`
        // witness is emitted, so one message states a map's remedy and a
        // struct's alike.
        if is_flattened(field) {
            if is_open(field) {
                continue;
            }
            return Err(syn::Error::new(
                span,
                format!(
                    "`{key}` on a flattened field is refused unless it is \
                     `#[schema(open)]`. A flattened map must be `#[schema(open)]` for the schema \
                     to describe it, and may then skip itself, since serde reads it absent as \
                     empty. A flattened struct is written whole or not at all, so drop `{key}` \
                     to keep its members consistent with its schema. `#[serde(default)]` does \
                     not change this on a flattened field"
                ),
            ));
        }

        if !is_required(field, &container) {
            continue;
        }
        return Err(syn::Error::new(
            span,
            format!(
                "`{key}` lets serde leave this field out of what it writes, but without a \
                 `#[serde(default)]` on the field or its struct serde still requires it on \
                 read, so no `required` list is true in both directions. Add \
                 `#[serde(default)]` beside it or on the struct, or make the field an `Option`"
            ),
        ));
    }
    Ok(())
}

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
fn reject_contradicted_closure(input: &DeriveInput) -> syn::Result<()> {
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
fn reject_closed_tagged_struct(input: &DeriveInput) -> syn::Result<()> {
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
fn reject_field_named_as_tag(input: &DeriveInput) -> syn::Result<()> {
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
/// which is all [`is_described`] names, and an object constraining no member it
/// does not name still admits what serde writes of it. A closed object
/// ([`closed`](super::closed)) constrains every such member, so it refuses what
/// serde writes of the field. Checked in every object serde writes, a struct
/// and each struct variant it writes. A `#[serde(transparent)]` struct is its
/// one field's value, with no object to check.
///
/// An open flattened field constrains such members only when its type hoists an
/// `additionalProperties`, which is its type's answer rather than its syntax's,
/// so that case is a bound
/// ([`open_fields_beside_unread_fields`](super::open_fields_beside_unread_fields))
/// rather than a refusal here.
fn reject_unread_field_in_closed_object(input: &DeriveInput) -> syn::Result<()> {
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

/// A member serde leaves out in one direction only has no one position to
/// describe.
///
/// A position is on the wire or not as a whole, and so is a newtype variant's
/// payload, which serde writes as a unit variant without it. So a tuple,
/// tuple-variant or newtype-variant member carrying `skip_serializing` or
/// `skip_deserializing` alone makes serde write one shape and read another.
/// `skip_serializing_if` does the same on a tuple member, except on the last
/// position beside `#[serde(default)]`, which serde fills when the array ends
/// early and [`min_items`](super::min_items) leaves out of the bound.
///
/// A newtype struct is never checked, since serde ignores all three there, and
/// neither is a newtype variant's `skip_serializing_if`. A
/// `#[serde(transparent)]` struct is described by its one field rather than as
/// an array, and a variant serde skips both ways is in no schema. A variant
/// serde never writes is only read, so there only a lone `skip_deserializing`
/// is refused.
fn reject_one_way_member_skip(input: &DeriveInput) -> syn::Result<()> {
    let container = Container::read(input);
    if container.transparent {
        return Ok(());
    }

    let groups: Vec<(&Punctuated<Field, Comma>, bool)> = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Unnamed(unnamed) if unnamed.unnamed.len() > 1 => {
                vec![(&unnamed.unnamed, true)]
            }
            Fields::Named(_) | Fields::Unnamed(_) | Fields::Unit => Vec::new(),
        },
        Data::Enum(data) => described_variants(data)
            .into_iter()
            .filter_map(|variant| match &variant.fields {
                Fields::Unnamed(unnamed) => Some((&unnamed.unnamed, is_written(variant))),
                Fields::Named(_) | Fields::Unit => None,
            })
            .collect(),
        // Refused at the top of `expand_inner`.
        Data::Union(_) => Vec::new(),
    };

    for (members, written) in groups {
        let keys: &[&str] = if written {
            &["skip_serializing", "skip_deserializing"]
        } else {
            &["skip_deserializing"]
        };
        let positions = positional_members(members);
        for (index, field) in positions.iter().enumerate() {
            if let Some((key, span)) = one_way_skip_span(field, keys) {
                return Err(syn::Error::new(
                    span,
                    format!(
                        "`{key}` leaves this member out in one direction only, and a tuple \
                         position or a newtype variant's payload is on the wire or not as a \
                         whole, so serde would write one shape and read another. Use \
                         `#[serde(skip)]` to leave it out both ways, or give the type named \
                         fields"
                    ),
                ));
            }

            if !written {
                continue;
            }
            let Some((_, span)) = serde_key_span(&field.attrs, &["skip_serializing_if"]) else {
                continue;
            };
            // A container default fills the end of a tuple struct as a
            // field-level one fills its own member.
            let defaulted = container.default || serde_flag(&field.attrs, &["default"]);
            let last = index + 1 == positions.len();
            if members.len() == 1 || (last && defaulted) {
                continue;
            }
            return Err(syn::Error::new(
                span,
                "`skip_serializing_if` on a tuple member is refused unless it is the last \
                 described member and carries `#[serde(default)]`. serde leaves the member out \
                 of the array it writes, which moves every later member into its position, and \
                 reads the shorter array back only when a default fills the end. Move the member \
                 last beside `#[serde(default)]`, or give the type named fields",
            ));
        }
    }
    Ok(())
}

/// A newtype variant of an adjacently tagged enum whose member serde skips has
/// no one schema, unless that member is an `Option`.
///
/// serde writes the variant as its tag alone, but reads it by its declared
/// newtype style rather than the unit style it wrote, so it demands the content
/// property and reads only `{"t":"V","c":null}`. An `Option` member reads the
/// missing content as `None`, so it round-trips as the tag-only branch `branch`
/// emits. External and internal tagging read back what they write, and a
/// member skipped one way only is refused before this is reached.
///
/// Checked on every variant the schema describes, including one serde reads
/// and never writes: serde still reads it only with its content, so the
/// tag-only branch would describe a request serde refuses.
fn reject_skipped_adjacent_payload(input: &DeriveInput) -> syn::Result<()> {
    let Data::Enum(data) = &input.data else {
        return Ok(());
    };
    let container = Container::read(input);
    let (Some(_), Some(_)) = (&container.tag, &container.content) else {
        return Ok(());
    };

    for variant in described_variants(data) {
        let Fields::Unnamed(unnamed) = &variant.fields else {
            continue;
        };
        let Some(member) = unnamed.unnamed.first() else {
            continue;
        };
        if !is_unit_like(&variant.fields) || is_option(&member.ty) {
            continue;
        }
        let keys = &["skip", "skip_serializing", "skip_deserializing"];
        let Some((key, span)) = serde_key_span(&member.attrs, keys) else {
            continue;
        };
        return Err(syn::Error::new(
            span,
            format!(
                "`{key}` leaves out the only member of a newtype variant in an adjacently \
                 tagged enum, so serde writes the variant as its tag alone, but reads it back \
                 only with its content present, which nothing serde writes carries. Make the \
                 member an `Option`, which serde reads absent, or `#[serde(skip)]` the whole \
                 variant"
            ),
        ));
    }
    Ok(())
}
