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
//! `kynos::schema::flatten::OpenMap`, or by `kynos::schema::flatten::AdmitsAny`
//! beside a field serde never reads, and every other flattened field by
//! `kynos::schema::flatten::Flatten`, and in an object
//! `#[serde(deny_unknown_fields)]` closes by
//! `kynos::schema::flatten::ClosedFlatten` as well.

mod aliases;
mod attributes;
mod refusals;
mod shape;

use attributes::{
    constraints, described_members, field_name, field_read_name, is_described, is_flattened,
    is_open, is_phantom, is_required, is_skipped_both_ways, is_unit_like, serde_flag,
    serde_key_span, sides, transparent_member, variant_read_name, variant_rename_all,
};
use shape::{enum_body, struct_body};

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, quote_spanned};
use syn::{
    Data, DataEnum, DeriveInput, Field, Fields, Lit, LitFloat, LitInt, LitStr, Type, Variant,
    ext::IdentExt, parse_macro_input, punctuated::Punctuated, spanned::Spanned, token::Comma,
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
    refusals::check(input)?;

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
            impl #impl_generics ::kynos::schema::flatten::Flatten
                for #name #ty_generics #where_clause {}
        }
    });
    let closed_flatten = flattens_into_closed_objects(input, &container).then(|| {
        quote! {
            #[allow(deprecated)]
            impl #impl_generics ::kynos::schema::flatten::ClosedFlatten
                for #name #ty_generics #where_clause {}
        }
    });

    Ok(quote! {
        #witnesses
        #flatten
        #closed_flatten

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
/// itself. `kynos::schema::flatten::Flatten` is the claim that it does not.
///
/// Asserted in a `const _` rather than as a predicate on the implementation,
/// for the reason the `ApiError` derive's `Display` witness gives: the
/// diagnostic lands on the type's own definition instead of on whatever
/// downstream code happens to name it. `schema_bounded_generics` also records
/// why field-type predicates were rejected once already.
///
/// A field carrying `#[schema(open)]` is bounded by
/// `kynos::schema::flatten::OpenMap` instead. That attribute is the declaration
/// that the object really is open, and `object_body` describes it by hoisting
/// the field's `additionalProperties` to `unevaluatedProperties` — which only a
/// map described in place has to hoist, since anything reached through a `$ref`
/// would carry its own into the `allOf`.
///
/// An internally tagged newtype variant's payload is bounded by `Flatten` too.
/// The variant has no properties of its own to put the tag beside, so its
/// payload is composed with a tag-only object in an `allOf` — a flatten in all
/// but the attribute, with the same thing to get wrong. A newtype variant whose
/// member serde skips is a unit on the wire and composes no payload, so its
/// member is bounded by nothing.
///
/// In an object `#[serde(deny_unknown_fields)]` closes ([`closed`]), a
/// flattened field is also bounded by `kynos::schema::flatten::ClosedFlatten`:
/// serde refuses every key no flattened field took, and only a type it reads by
/// name takes one ([`flattens_into_closed_objects`]). `Flatten` stays asserted
/// beside it, since `ClosedFlatten` implies it and a type that is not
/// flattenable at all is then refused with that reason too. The payload of an
/// internally tagged newtype variant is bounded by `Flatten` alone, since its
/// tag-only object is never closed.
fn flatten_witnesses(
    input: &DeriveInput,
    container: &Container,
    generics: &syn::Generics,
) -> TokenStream2 {
    let (impl_generics, _, where_clause) = generics.split_for_impl();

    let closing = container.deny_unknown_fields && !container.transparent;
    let flattened = described_groups(input)
        .into_iter()
        .flat_map(described_members)
        .filter(|field| is_flattened(field))
        .map(|field| (field, closing));

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

    let admitting = open_fields_beside_unread_fields(input, container);

    let payloads = payloads.into_iter().map(|field| (field, false));
    let witnesses = flattened.chain(payloads).map(|(field, closed)| {
        let ty = &field.ty;
        // Spanned at the field's type, so the refusal points at what was
        // written rather than at the derive.
        if admitting.iter().any(|open| std::ptr::eq(*open, field)) {
            quote_spanned! {ty.span()=>
                const _: () = {
                    #[allow(dead_code, deprecated)]
                    fn open_fields_beside_unread_fields_admit_any_member #impl_generics ()
                        #where_clause
                    {
                        fn admits_any<T: ::kynos::schema::flatten::AdmitsAny + ?Sized>() {}
                        admits_any::<#ty>();
                    }
                };
            }
        } else if is_open(field) {
            quote_spanned! {ty.span()=>
                const _: () = {
                    #[allow(dead_code, deprecated)]
                    fn open_fields_are_maps_described_in_place #impl_generics () #where_clause {
                        fn is_open_map<T: ::kynos::schema::flatten::OpenMap + ?Sized>() {}
                        is_open_map::<#ty>();
                    }
                };
            }
        } else {
            let read_by_name = closed.then(|| {
                quote_spanned! {ty.span()=>
                    const _: () = {
                        #[allow(dead_code, deprecated)]
                        fn closed_objects_flatten_what_serde_reads_by_name #impl_generics ()
                            #where_clause
                        {
                            fn is_closed_flattenable<
                                T: ::kynos::schema::flatten::ClosedFlatten + ?Sized,
                            >() {}
                            is_closed_flattenable::<#ty>();
                        }
                    };
                }
            });
            quote_spanned! {ty.span()=>
                const _: () = {
                    #[allow(dead_code, deprecated)]
                    fn flattened_fields_name_their_members #impl_generics () #where_clause {
                        fn is_flattenable<T: ::kynos::schema::flatten::Flatten + ?Sized>() {}
                        is_flattenable::<#ty>();
                    }
                };
                #read_by_name
            }
        }
    });

    quote!(#(#witnesses)*)
}

/// The open flattened fields that sit beside a named field serde writes and
/// never reads, `skip_deserializing` alone, in an object serde writes.
///
/// The schema leaves such a field out, so the object must not constrain the
/// members it does not name, and an open field that hoists an
/// `additionalProperties` would. Whether this one does is its type's answer,
/// invisible here, so [`flatten_witnesses`] bounds it by
/// `kynos::schema::flatten::AdmitsAny` rather than by `OpenMap`, which it
/// implies. Read over the objects
/// `refusals::reject_unread_field_in_closed_object` reads: a
/// `#[serde(transparent)]` struct is its one field's value, with no object to
/// bound, and an object `#[serde(deny_unknown_fields)]` closes never reaches
/// here holding an open field, which `refusals::reject_contradicted_closure`
/// refuses.
fn open_fields_beside_unread_fields<'a>(
    input: &'a DeriveInput,
    container: &Container,
) -> Vec<&'a Field> {
    if container.transparent {
        return Vec::new();
    }
    written_groups(input)
        .into_iter()
        .filter(|fields| unread_field_span(fields).is_some())
        .flat_map(described_members)
        .filter(|field| is_open(field))
        .collect()
}

/// The field groups of every object serde writes: a struct's, and each
/// variant's serde writes.
fn written_groups(input: &DeriveInput) -> Vec<&Fields> {
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

/// Whether serde reads this [`flattens`] shape by name, so it may be flattened
/// into an object `#[serde(deny_unknown_fields)]` closes.
///
/// Such a parent refuses every key no flattened field took, and serde takes a
/// key only through `deserialize_struct`, which claims the keys it names. It
/// reads a struct that way unless a field is flattened without
/// `skip_deserializing` — serde's own test, so a flattened `PhantomData`
/// counts — in which case it reads the struct as a map. A struct carrying a
/// container `#[serde(tag = "...")]` is excluded too: serde writes the tag
/// beside the fields and never names it among the keys it takes. An adjacently
/// tagged enum names its tag and content; an internally tagged one reads
/// through `deserialize_any`, which takes nothing.
fn flattens_into_closed_objects(input: &DeriveInput, container: &Container) -> bool {
    if !flattens(input, container) {
        return false;
    }
    match &input.data {
        Data::Struct(data) => {
            container.tag.is_none()
                && !data.fields.iter().any(|field| {
                    is_flattened(field)
                        && !serde_flag(&field.attrs, &["skip", "skip_deserializing"])
                })
        }
        Data::Enum(_) => container.content.is_some(),
        Data::Union(_) => false,
    }
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
/// [`refusals`]' `check_constraints` still reads [`field_groups`], because a
/// malformed attribute is an error wherever it is written.
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
/// refused by `refusals::reject_unread_variant` before any of these is read.
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
/// Read off skip attributes rather than [`is_described`], which would drop a
/// member carrying `skip_deserializing` alone: serde still writes that member
/// into its position, and `refusals::reject_one_way_member_skip` has to see it
/// to refuse it.
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
/// `refusals::reject_one_way_member_skip` accepts only beside a default
/// wherever serde writes the tuple; in a variant serde never writes, such a
/// member without a default still counts, since serde reads every position it
/// does not fill.
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
#[derive(Clone, Default)]
struct Container {
    /// The container `rename`, on the serialize side where it is split: the
    /// name serde writes a struct's `#[serde(tag = "...")]` as.
    rename: Option<String>,
    /// The container `rename_all` style, on the serialize side where it is
    /// split. [`split_rule`] finds sides that differ, which this derive and
    /// [`multipart`](super::multipart) refuse before any name is taken from
    /// it, so it is the style of both directions.
    ///
    /// It names the members this container describes directly: a struct's
    /// fields, an enum's variants, and in [`Container::fields_of`] a variant's
    /// fields.
    rename_all: Option<String>,
    /// An enum's `rename_all_fields` style, read as `rename_all` is: the rule
    /// serde names a variant's fields by where the variant has no
    /// `rename_all` of its own.
    rename_all_fields: Option<String>,
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
                    "rename" => container.rename = sides(&meta)?.serialize,
                    "rename_all" => container.rename_all = sides(&meta)?.serialize,
                    "rename_all_fields" => {
                        container.rename_all_fields = sides(&meta)?.serialize;
                    }
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

    /// The container a variant's fields are named and described under: this
    /// one, with the variant's own `rename_all`, else the enum's
    /// `rename_all_fields`, side by side, in place of the enum's `rename_all`,
    /// which serde applies to variant names alone (`serde_derive` 1.0.229,
    /// `internals/ast.rs`).
    ///
    /// The side is the one serde uses the variant's fields on: the serialize
    /// side for a variant serde writes, and the deserialize side for one it
    /// only reads. `refusals::reject_split_rename_all` refuses a struct
    /// variant's rule whose sides differ where serde uses both, and a split
    /// `rename_all_fields`
    /// reaching any struct variant, so the serialize side read into
    /// [`Container::rename_all_fields`] is its deserialize side too.
    fn fields_of(&self, variant: &Variant) -> Self {
        let own = variant_rename_all(variant);
        let own = if is_written(variant) {
            own.serialize
        } else {
            own.deserialize
        };
        Self {
            rename_all: own.or_else(|| self.rename_all_fields.clone()),
            ..self.clone()
        }
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
        Data::Struct(data) => {
            let name = container
                .rename
                .clone()
                .unwrap_or_else(|| input.ident.unraw().to_string());
            described(
                struct_body(&data.fields, container, &name),
                container.doc.as_deref(),
            )
        }
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
/// the spelling every consumer reads. `unevaluatedProperties: false` where the
/// object carries an `allOf`, which a flattened field composes members through
/// and an aliased field bounds its names in, since `additionalProperties` sees
/// only the object's own `properties` and would refuse a flattened field's
/// members, while `unevaluatedProperties` sees them across the `allOf` and any
/// `$ref` inside it. serde closes a struct, every struct variant's fields,
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

/// The span of the first field whose split `rename` gives serde's two
/// directions different names, at that `rename`.
///
/// Read by [`multipart`](super::multipart), whose part carries one name in both
/// directions, so no side of such a rename is the part's name.
pub(super) fn split_renamed_field(input: &DeriveInput, fields: &syn::FieldsNamed) -> Option<Span> {
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
/// whose sides differ, one side left out included, at that `key`.
///
/// Read by this derive's refusal of a split container `rename_all`, enum
/// `rename_all_fields` or variant `rename_all`, and by
/// [`multipart`](super::multipart)'s of a split container `rename_all`, whose
/// part carries one name in both directions. Shape errors in the list stay
/// serde's to report.
pub(super) fn split_rule(attrs: &[syn::Attribute], key: &str) -> Option<Span> {
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
