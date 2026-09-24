//! `#[derive(Schema)]`.
//!
//! The constraint half of the field grammar is exactly the keys of
//! `schema::constraints::Constraints`, so the attribute and the type it fills
//! are one list and neither can grow without the other:
//!
//! ```text
//! #[schema( <member> [, <member>]* )]             on a field, optional
//!
//! member := <constraint> | open
//!
//! constraint := minimum = <number> | maximum = <number>
//!             | exclusive_minimum = <number> | exclusive_maximum = <number>
//!             | multiple_of = <number>
//!             | min_length = <integer> | max_length = <integer>
//!             | pattern = "<regex>"
//!             | min_items = <integer> | max_items = <integer>
//!             | unique_items
//! ```
//!
//! `format` is deliberately absent. It states what a value *is*, which follows
//! from the type or from nothing, so naming it here is an error that points at
//! the remedy rather than a key that quietly works.
//!
//! `open` is the one member that is not a constraint, which is why the list is
//! no longer the `Constraints` keys alone. It says how a `#[serde(flatten)]`
//! field composes rather than what a value may be. An open field is bounded by
//! `kynos::schema::OpenMap`, and every other flattened field by
//! `kynos::schema::Flatten`.

mod attributes;
mod shape;

use attributes::{
    constraints, described_members, field_name, is_described, is_flattened, is_open, is_option,
    is_phantom, is_required, is_skipped_both_ways, is_unit_like, open_span, serde_flag,
    serde_key_span, transparent_member, transparent_members, variant_name,
};
use shape::{enum_body, struct_body};

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, quote_spanned};
use syn::{
    Data, DataEnum, DeriveInput, Field, Fields, Lit, LitFloat, LitInt, LitStr, Type, Variant,
    parse_macro_input, punctuated::Punctuated, spanned::Spanned, token::Comma,
};

use crate::derive::common::{doc_string, is_deprecated, skip_value};

/// Keys taking a number, which may be written as an integer or a float.
const NUMERIC: &[&str] = &[
    "minimum",
    "maximum",
    "exclusive_minimum",
    "exclusive_maximum",
    "multiple_of",
];

/// Keys taking a non-negative count.
const COUNTS: &[&str] = &["min_length", "max_length", "min_items", "max_items"];

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

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

pub(super) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    if let Data::Union(data) = &input.data {
        return Err(syn::Error::new(
            data.union_token.span(),
            "`Schema` cannot describe a union: no JSON value corresponds to one",
        ));
    }
    reject_container_conversions(input)?;
    reject_untagged(input)?;
    reject_unread_variant(input)?;
    reject_wire_form_overrides(input)?;
    reject_catch_all(input)?;
    reject_transparent_without_one_field(input)?;
    reject_read_required_skip(input)?;
    reject_contradicted_closure(input)?;
    reject_unread_field_in_closed_object(input)?;
    reject_one_way_member_skip(input)?;
    reject_skipped_adjacent_payload(input)?;
    check_constraints(input)?;

    let name = &input.ident;
    let generics = schema_bounded_generics(input);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Only a concrete type claims a component name. A generic one would give
    // every instantiation the same name, so `Page<User>` and `Page<Order>`
    // would collide in `components`; mangling the arguments into a legal
    // component key is the eventual answer, and inlining is the honest
    // placeholder rather than a name that is wrong.
    let component = LitStr::new(&name.to_string(), name.span());
    let named = if input.generics.type_params().next().is_some() {
        quote!(::core::option::Option::None)
    } else {
        quote!(::kynos::openapi::ComponentName::sanitized(#component).ok())
    };

    let container = Container::read(input);
    let body = body(input, &container);
    let witnesses = flatten_witnesses(input, &container, &generics);
    let flatten = flattens(input, &container).then(|| {
        quote! {
            #[allow(deprecated)]
            impl #impl_generics ::kynos::schema::Flatten for #name #ty_generics #where_clause {}
        }
    });

    Ok(quote! {
        #witnesses
        #flatten

        // A deprecated type still has to describe itself, and the impl below
        // names it. Without this, `#[deprecated]` plus `#[derive(Schema)]` is a
        // warning at the type's own definition -- an error under `-D warnings`,
        // which this workspace and many others set. serde's derives carry the
        // same allow for the same reason.
        #[allow(deprecated)]
        impl #impl_generics ::kynos::schema::Schema for #name #ty_generics #where_clause {
            fn schema(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::kynos::openapi::Schema {
                #body
            }

            fn name() -> ::core::option::Option<::kynos::openapi::ComponentName> {
                #named
            }
        }
    })
}

/// The input's generics, with `Schema` required of each type parameter.
///
/// serde's own default shape, and for the same reason: bounding the
/// *parameters* rather than the field types is both sufficient and narrower.
/// `Vec<T>: Schema` follows from `T: Schema` through the blanket
/// implementation, while a field-type bound would demand `PhantomData<T>:
/// Schema` — a bound nothing satisfies, on a field described as the `null`
/// serde writes without it, failing at the handler rather than here.
///
/// Emitted now because the implementation will need it, and adding a bound
/// after the freeze breaks exactly the code this milestone invites people to
/// write.
fn schema_bounded_generics(input: &DeriveInput) -> syn::Generics {
    let mut generics = input.generics.clone();
    let parameters: Vec<syn::Ident> = generics
        .type_params()
        .map(|parameter| parameter.ident.clone())
        .collect();
    if parameters.is_empty() {
        return generics;
    }

    let clause = generics.make_where_clause();
    for parameter in parameters {
        clause
            .predicates
            .push(syn::parse_quote!(#parameter: ::kynos::schema::Schema));
    }
    generics
}

/// One witness per flattened field, requiring that its type names its members.
///
/// A flattened field's members become the parent's own, so the parent composes
/// the field's schema rather than naming it — and a composed schema that
/// constrains every member it does not name, which is what a map's
/// `additionalProperties` is, then reaches the members the parent declared
/// itself. `kynos::schema::Flatten` is the claim that it does not.
///
/// Asserted in a `const _` rather than as a predicate on the implementation,
/// for the reason the `ApiError` derive's `Display` witness gives: the
/// diagnostic lands on the type's own definition instead of on whatever
/// downstream code happens to name it. `schema_bounded_generics` also records
/// why field-type predicates were rejected once already.
///
/// A field carrying `#[schema(open)]` is bounded by `kynos::schema::OpenMap`
/// instead. That attribute is the declaration that the object really is open,
/// and `object_body` describes it by hoisting the field's `additionalProperties`
/// to `unevaluatedProperties` — which only a map described in place has to
/// hoist, since anything reached through a `$ref` would carry its own into the
/// `allOf`.
///
/// An internally tagged newtype variant's payload is bounded by `Flatten` too.
/// The variant has no properties of its own to put the tag beside, so its
/// payload is composed with a tag-only object in an `allOf` — a flatten in all
/// but the attribute, with the same thing to get wrong. A newtype variant whose
/// member serde skips is a unit on the wire and composes no payload, so its
/// member is bounded by nothing.
fn flatten_witnesses(
    input: &DeriveInput,
    container: &Container,
    generics: &syn::Generics,
) -> TokenStream2 {
    let (impl_generics, _, where_clause) = generics.split_for_impl();

    let flattened = described_groups(input)
        .into_iter()
        .flat_map(described_members)
        .filter(|field| is_flattened(field));

    let payloads: Vec<&Field> = match (&input.data, &container.tag, &container.content) {
        (Data::Enum(data), Some(_), None) => described_variants(data)
            .into_iter()
            .filter(|variant| !is_unit_like(&variant.fields))
            .filter_map(|variant| match &variant.fields {
                Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => unnamed.unnamed.first(),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };

    let witnesses = flattened.chain(payloads).map(|field| {
        let ty = &field.ty;
        // Spanned at the field's type, so the refusal points at what was
        // written rather than at the derive.
        if is_open(field) {
            quote_spanned! {ty.span()=>
                const _: () = {
                    #[allow(dead_code, deprecated)]
                    fn open_fields_are_maps_described_in_place #impl_generics () #where_clause {
                        fn is_open_map<T: ::kynos::schema::OpenMap + ?Sized>() {}
                        is_open_map::<#ty>();
                    }
                };
            }
        } else {
            quote_spanned! {ty.span()=>
                const _: () = {
                    #[allow(dead_code, deprecated)]
                    fn flattened_fields_name_their_members #impl_generics () #where_clause {
                        fn is_flattenable<T: ::kynos::schema::Flatten + ?Sized>() {}
                        is_flattenable::<#ty>();
                    }
                };
            }
        }
    });

    quote!(#(#witnesses)*)
}

/// Whether the schema this input emits names its own members, and so may itself
/// be flattened.
///
/// True of the shapes whose description is an object whose `properties` names
/// every member it admits: a struct with named fields, and an enum whose every
/// `oneOf` branch is such an object. A newtype, a tuple and a unit struct are
/// not objects at all; an internally tagged newtype variant composes with
/// whatever its payload resolves to, which is exactly the unknown this trait
/// exists to refuse; and an externally tagged enum is excluded whatever its
/// variants. serde reads each of its object branches as exactly one entry, so
/// the branch is closed to every key but the variant's (`branch`), and a unit
/// variant is a bare string.
///
/// A container carrying `#[schema(open)]` is excluded whatever its shape: its
/// own `unevaluatedProperties` would, one level up, reach the members the outer
/// object declared. So is a `#[serde(transparent)]` one, whose wire form is its
/// one field's value rather than an object naming the fields it declares. So is
/// a struct, or an internally tagged enum with a struct variant serde writes,
/// holding a named field serde writes and never reads ([`unread_field_span`]):
/// its schema leaves out a member serde writes beside the ones it names, which
/// an open map one level up would refuse.
///
/// So is every object [`closed`] closes under `#[serde(deny_unknown_fields)]`,
/// for the reason an open container is excluded: a struct, an internally tagged
/// enum with a struct variant, and an adjacently tagged enum. An internally
/// tagged unit variant stays open, since serde ignores every key beside its
/// tag.
fn flattens(input: &DeriveInput, container: &Container) -> bool {
    if container.transparent {
        return false;
    }

    let groups = described_groups(input);
    if groups.iter().flat_map(|group| group.iter()).any(is_open) {
        return false;
    }

    match &input.data {
        Data::Struct(data) => {
            matches!(data.fields, Fields::Named(_))
                && unread_field_span(&data.fields).is_none()
                && !container.deny_unknown_fields
        }
        Data::Enum(data) => {
            let variants = described_variants(data);

            match (&container.tag, &container.content) {
                // Adjacently tagged: every branch is an object of a tag
                // property and a content property, whatever the variant holds.
                (Some(_), Some(_)) => !variants.is_empty() && !container.deny_unknown_fields,
                // Internally tagged: a named or unit variant becomes an object
                // naming its own members plus the tag, and serde writes a
                // variant's fields beside that tag, so one it never reads is a
                // member the object does not name.
                (Some(_), None) => {
                    !variants.is_empty()
                        && variants.iter().all(|variant| match variant.fields {
                            Fields::Unit => true,
                            Fields::Named(_) => !container.deny_unknown_fields,
                            Fields::Unnamed(_) => false,
                        })
                        && variants
                            .iter()
                            .filter(|variant| is_written(variant))
                            .all(|variant| unread_field_span(&variant.fields).is_none())
                }
                // Externally tagged: a unit variant is a bare string, and every
                // other branch is closed to all but its variant key, which one
                // level up would refuse the outer object's own members.
                (None, _) => false,
            }
        }
        // Refused at the top of `expand_inner`.
        Data::Union(_) => false,
    }
}

/// Validates every `#[schema(...)]` in the input.
///
/// Run before any code is emitted, so that [`constraints`] can read the same
/// lists back without checking them again — a key that reached the emitter had
/// its shape settled here, and one that did not never gets there.
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

/// Each group of fields the input declares that becomes one emitted object.
///
/// A struct has one; an enum has one per variant, because a variant's fields
/// are composed into a branch of their own.
fn field_groups(input: &DeriveInput) -> Vec<&Fields> {
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
/// serde neither writes nor reads such a variant, so no branch is emitted for it
/// and its fields reach neither a flatten witness nor the `Flatten` decision.
/// `check_constraints` still reads [`field_groups`], because a malformed
/// attribute is an error wherever it is written.
fn described_groups(input: &DeriveInput) -> Vec<&Fields> {
    match &input.data {
        Data::Enum(data) => described_variants(data)
            .into_iter()
            .map(|variant| &variant.fields)
            .collect(),
        _ => field_groups(input),
    }
}

/// The variants the emitted schema describes: every one serde does not skip
/// both ways.
///
/// A variant serde reads and never writes is among them, since a request
/// carrying it is one serde accepts. One serde writes and never reads is
/// refused by [`reject_unread_variant`] before any of these is read.
fn described_variants(data: &DataEnum) -> Vec<&Variant> {
    data.variants
        .iter()
        .filter(|variant| !is_skipped_both_ways(&variant.attrs))
        .collect()
}

/// Whether serde writes a variant: it carries neither `skip` nor
/// `skip_serializing`.
///
/// serde's `Serialize` arm for any other variant errors before it touches a
/// field, so an attribute deciding only the written form changes nothing there.
fn is_written(variant: &Variant) -> bool {
    !serde_flag(&variant.attrs, &["skip", "skip_serializing"])
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
/// everywhere serde accepts the three keys. A named field serde never reads, a
/// flattened `PhantomData` and every field of a variant serde skips both ways
/// are in no schema, so an override on one of them contradicts nothing and is
/// left alone. Any other `PhantomData` is described as `null`, which an
/// override contradicts as it would any other type's schema. Where serde never writes -- inside a variant serde never writes, and
/// on a named field carrying `skip_serializing` alone -- only
/// [`READ_OVERRIDES`] are refused: [`is_written`] says why `serialize_with`
/// changes nothing there.
///
/// A `#[serde(transparent)]` struct is scanned over the fields
/// [`transparent_members`] says serde writes through, for [`WRITE_OVERRIDES`],
/// and reads through, for [`READ_OVERRIDES`]: serde hands no other field's value
/// to a function in either direction.
///
/// An unnamed member is exempt only when serde skips it both ways, and never on
/// a newtype struct, whose member serde writes through the function whatever it
/// skips. Skip attributes are read rather than [`is_described`], which would
/// exempt that newtype member.
fn reject_wire_form_overrides(input: &DeriveInput) -> syn::Result<()> {
    type Scanned<'a> = (&'a [syn::Attribute], &'static str, &'static [&'static str]);

    fn fields<'a>(
        fields: &'a Fields,
        newtype: bool,
        keys: &'static [&'static str],
    ) -> Vec<Scanned<'a>> {
        let described: fn(&&Field) -> bool = match fields {
            Fields::Named(_) => |field| is_described(field),
            Fields::Unnamed(_) if newtype => |_| true,
            Fields::Unnamed(_) | Fields::Unit => |field| !is_skipped_both_ways(&field.attrs),
        };
        fields
            .iter()
            .filter(described)
            .map(|field| {
                // A described named field is read, so `skip_serializing` on one
                // is `skip_serializing` alone.
                let unwritten =
                    field.ident.is_some() && serde_flag(&field.attrs, &["skip_serializing"]);
                let keys = if unwritten { READ_OVERRIDES } else { keys };
                (field.attrs.as_slice(), "field", keys)
            })
            .collect()
    }

    fn picked(fields: &Fields) -> Vec<Scanned<'_>> {
        let (written, read) = transparent_members(fields);
        let among = |members: &[&Field], field: &Field| {
            members.iter().any(|member| std::ptr::eq(*member, field))
        };
        fields
            .iter()
            .filter_map(|field| {
                let keys = match (among(&written, field), among(&read, field)) {
                    (true, true) => WIRE_FORM_OVERRIDES,
                    (true, false) => WRITE_OVERRIDES,
                    (false, true) => READ_OVERRIDES,
                    (false, false) => return None,
                };
                Some((field.attrs.as_slice(), "field", keys))
            })
            .collect()
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
/// serde picks per direction, from the attributes alone ([`transparent_members`]):
/// it writes through the field without `skip` or `skip_serializing`, reads
/// through the field without `skip`, `skip_deserializing` or a field-level
/// `default`, and never through a `PhantomData`. Where each direction picks a
/// single field and they are different fields, the struct is refused: the
/// derive compares fields rather than their schemas, so two fields of one type
/// are refused too. Everything else is [`transparent_member`]'s, which `struct_body`
/// describes. Where only one direction picks a single field, serde refuses the
/// other derive by itself, so the struct compiles with that direction's derive
/// alone and the field is true of it. Where neither does, serde refuses the
/// struct for either derive, and a second error here would restate it; that
/// covers a unit struct too, and an enum is serde's to refuse.
fn reject_transparent_without_one_field(input: &DeriveInput) -> syn::Result<()> {
    let Data::Struct(data) = &input.data else {
        return Ok(());
    };
    let Some((_, span)) = serde_key_span(&input.attrs, &["transparent"]) else {
        return Ok(());
    };
    let (written, read) = transparent_members(&data.fields);
    let ([written], [read]) = (written.as_slice(), read.as_slice()) else {
        return Ok(());
    };
    if std::ptr::eq(*written, *read) {
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
/// flattened open map or an `alias` it reads.
///
/// serde refuses every key no field it reads names before a flattened map sees
/// it, so the map reads empty while serde writes its members; and it reads an
/// aliased field under a name the closed object does not name. Checked on every
/// named field the schema describes, including inside a variant serde never
/// writes, since the object is closed on read alone. A `#[serde(transparent)]`
/// struct is its one field's value, with no object to close.
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
        if let Some((_, span)) = serde_key_span(&field.attrs, &["alias"]) {
            return Err(syn::Error::new(
                span,
                "`alias` makes serde read this field under a second name, and the object \
                 `#[serde(deny_unknown_fields)]` closes names only the first, so the schema would \
                 refuse a document serde reads. Drop the `alias`, or drop `deny_unknown_fields`",
            ));
        }
    }
    Ok(())
}

/// A named field serde writes and never reads is refused in an object that
/// constrains every member it does not name.
///
/// `skip_deserializing` alone keeps the field out of the object serde reads,
/// which is all [`is_described`] names, and an object constraining no member it
/// does not name still admits what serde writes of it. Two things make the
/// object constrain such members. An open flattened field gives the object an
/// `unevaluatedProperties` that reaches the field, and refuses what serde writes
/// wherever the map's values are another type, which is invisible here.
/// `#[serde(deny_unknown_fields)]` closes the object outright ([`closed`]).
/// Checked in every object serde writes, a struct and each struct variant it
/// writes, against an open field the schema describes or a closed container. A
/// `#[serde(transparent)]` struct is its one field's value, with no object to
/// check.
fn reject_unread_field_in_closed_object(input: &DeriveInput) -> syn::Result<()> {
    let container = Container::read(input);
    if container.transparent {
        return Ok(());
    }
    let closer = if container.deny_unknown_fields {
        "the `additionalProperties` or `unevaluatedProperties` of `false` that \
         `#[serde(deny_unknown_fields)]` gives this object"
    } else {
        "the `unevaluatedProperties` a `#[schema(open)]` flattened field gives this object"
    };

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

    for fields in groups {
        if !container.deny_unknown_fields && !described_members(fields).into_iter().any(is_open) {
            continue;
        }
        if let Some(span) = unread_field_span(fields) {
            return Err(syn::Error::new(
                span,
                format!(
                    "`skip_deserializing` leaves this field out of the schema, since serde never \
                     reads it, but serde still writes it, and {closer} refuses a member the \
                     schema does not name. Use `#[serde(skip)]` to leave it out both ways, or \
                     drop `skip_deserializing` so the schema names it"
                ),
            ));
        }
    }
    Ok(())
}

/// Where the first named field serde writes and never reads, `skip_deserializing`
/// alone, carries that key.
fn unread_field_span(fields: &Fields) -> Option<Span> {
    let Fields::Named(named) = fields else {
        return None;
    };
    named
        .named
        .iter()
        .find_map(|field| one_way_skip_span(field, &["skip_deserializing"]))
        .map(|(_, span)| span)
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
/// early and [`min_items`] leaves out of the bound.
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

/// The first of `keys` a member carries without the other half of
/// `#[serde(skip)]`, and where it is written.
fn one_way_skip_span(field: &Field, keys: &[&str]) -> Option<(String, Span)> {
    if is_skipped_both_ways(&field.attrs) {
        return None;
    }
    serde_key_span(&field.attrs, keys)
}

/// The members of a tuple or tuple variant that hold a position on the wire,
/// in order: each one serde does not skip both ways.
///
/// Read off skip attributes rather than [`is_described`], because a
/// `PhantomData` dropped from the list would shift every later position.
fn positional_members(fields: &Punctuated<Field, Comma>) -> Vec<&Field> {
    fields
        .iter()
        .filter(|field| !is_skipped_both_ways(&field.attrs))
        .collect()
}

/// The fewest elements serde reads for a tuple holding these positions.
///
/// Every position up to the last one with no `#[serde(default)]`: serde fills a
/// defaulted member when the array ends before it, but a default ahead of a
/// member without one fills nothing, since the array cannot end there. That
/// covers the last position carrying `skip_serializing_if`, which
/// [`reject_one_way_member_skip`] accepts only beside a default wherever serde
/// writes the tuple; in a variant serde never writes, such a member without a
/// default still counts, since serde reads every position it does not fill.
/// Under a container default, `defaulted`, serde fills every missing trailing
/// element, so it reads the empty array and there is no bound.
fn min_items(positions: &[&Field], defaulted: bool) -> u64 {
    if defaulted {
        return 0;
    }
    let required = positions
        .iter()
        .rposition(|field| !serde_flag(&field.attrs, &["default"]))
        .map_or(0, |last| last + 1);
    u64::try_from(required).unwrap_or(u64::MAX)
}

/// What the type's own serde attributes said.
///
/// Read rather than restated: `rename_all`, `tag` and `content` are already on
/// the type because it has to serialize, and a parallel `#[schema(...)]`
/// spelling of them would be a second declaration to keep in step.
#[derive(Default)]
struct Container {
    rename_all: Option<String>,
    tag: Option<String>,
    content: Option<String>,
    /// `#[serde(transparent)]`: the wire form is the one field's value, not an
    /// object of the fields the declaration names, so `struct_body` describes
    /// that field and `flattens` refuses the `Flatten` claim.
    transparent: bool,
    doc: Option<String>,
    /// A container `#[serde(default)]`, which serde fills every missing field
    /// from, and on a tuple struct every missing trailing element. serde
    /// accepts it only on a struct, so it is never set for an enum.
    default: bool,
    /// `#[serde(deny_unknown_fields)]`: serde refuses a key naming no field it
    /// reads, so [`closed`] closes each object that rule reaches.
    deny_unknown_fields: bool,
}

impl Container {
    fn read(input: &DeriveInput) -> Self {
        let mut container = Self {
            doc: doc_string(&input.attrs),
            ..Self::default()
        };

        for attr in &input.attrs {
            if !attr.path().is_ident("serde") {
                continue;
            }
            // Shape errors in serde's own attribute are serde's to report:
            // this derive reads what it recognizes and stays silent about the
            // rest, so a key it has not learned is not a second diagnostic on
            // the same line.
            let _ = attr.parse_nested_meta(|meta| {
                let Some(key) = meta.path.get_ident() else {
                    return skip_value(&meta);
                };
                match key.to_string().as_str() {
                    "rename_all" => container.rename_all = string_value(&meta)?,
                    "tag" => container.tag = string_value(&meta)?,
                    "content" => container.content = string_value(&meta)?,
                    "transparent" => container.transparent = true,
                    "deny_unknown_fields" => container.deny_unknown_fields = true,
                    _ => skip_value(&meta)?,
                }
                Ok(())
            });
        }

        container.default = matches!(&input.data, Data::Struct(data) if !matches!(data.fields, Fields::Unit))
            && serde_flag(&input.attrs, &["default"]);

        container
    }
}

/// The `= "..."` of a nested-meta item, when it has one.
fn string_value(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<Option<String>> {
    if !meta.input.peek(syn::Token![=]) {
        return Ok(None);
    }
    Ok(Some(meta.value()?.parse::<LitStr>()?.value()))
}

/// The `schema` body for whatever shape the type has.
fn body(input: &DeriveInput, container: &Container) -> TokenStream2 {
    let described = match &input.data {
        Data::Struct(data) => described(
            struct_body(&data.fields, container),
            container.doc.as_deref(),
        ),
        Data::Enum(data) => described(enum_body(data, container), container.doc.as_deref()),
        // Refused at the top of `expand_inner`.
        Data::Union(_) => quote!(::kynos::openapi::Schema::default()),
    };

    deprecate(described, is_deprecated(&input.attrs))
}

/// Marks the schema deprecated, where the item said so and the schema can say it.
///
/// Shaped like [`described`], and for the same reason: a boolean schema has
/// nowhere to carry a keyword, so it carries none. A `$ref` does -- from 3.1
/// onward a schema `$ref` applies its siblings -- which is what lets a
/// deprecated field whose type is a named component be marked at the field
/// rather than on the component every other field shares.
///
/// Never `Some(false)`. The specification defaults the keyword to false, so
/// writing it out states nothing and puts a word in every schema in the
/// document; `Operation::set_deprecated` already takes the same line.
fn deprecate(schema: TokenStream2, deprecated: bool) -> TokenStream2 {
    if !deprecated {
        return schema;
    }
    quote! {
        {
            let mut deprecated = #schema;
            if let ::kynos::openapi::Schema::Object(keywords) = &mut deprecated {
                keywords.deprecated = ::core::option::Option::Some(true);
            }
            deprecated
        }
    }
}

/// Closes an object serde reads under `#[serde(deny_unknown_fields)]`, which
/// refuses every key naming no field it reads.
///
/// `additionalProperties: false` where the object composes nothing, which is
/// the spelling every consumer reads. `unevaluatedProperties: false` where a
/// flattened field composes members through an `allOf`, since
/// `additionalProperties` sees only the object's own `properties` and would
/// refuse them, while `unevaluatedProperties` sees them across the `allOf` and
/// any `$ref` inside it. serde closes a struct, every struct variant's fields,
/// and an adjacently tagged branch; a caller wraps exactly those objects, and
/// the schema is returned as it was without the attribute.
fn closed(schema: TokenStream2, container: &Container) -> TokenStream2 {
    if !container.deny_unknown_fields {
        return schema;
    }
    close(&schema)
}

/// Closes an object whatever the container says, as [`closed`] does under
/// `#[serde(deny_unknown_fields)]`: for an object serde closes on its own.
fn close(schema: &TokenStream2) -> TokenStream2 {
    quote! {
        {
            let mut closed = #schema;
            if let ::kynos::openapi::Schema::Object(keywords) = &mut closed {
                let never = ::core::option::Option::Some(::std::boxed::Box::new(
                    ::kynos::openapi::Schema::never(),
                ));
                if keywords.all_of.is_some() {
                    keywords.unevaluated_properties = never;
                } else {
                    keywords.additional_properties = never;
                }
            }
            closed
        }
    }
}

/// Attaches the type's own prose to the schema it produces.
fn described(schema: TokenStream2, doc: Option<&str>) -> TokenStream2 {
    let Some(doc) = doc else {
        return schema;
    };
    quote! {
        {
            let mut described = #schema;
            if let ::kynos::openapi::Schema::Object(keywords) = &mut described {
                keywords.description =
                    ::core::option::Option::Some(::std::string::String::from(#doc));
            }
            described
        }
    }
}

/// The names this derive would describe a struct's fields under, in order.
///
/// Read by [`multipart`](super::multipart), so that the part a body carries and
/// the property the description names come from one rule rather than two that
/// agree until a `rename_all` is added.
pub(super) fn property_names(input: &DeriveInput, fields: &syn::FieldsNamed) -> Vec<String> {
    let container = Container::read(input);
    fields
        .named
        .iter()
        .map(|field| field_name(field, &container))
        .collect()
}
