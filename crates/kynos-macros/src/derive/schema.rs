//! `#[derive(Schema)]`.
//!
//! The constraint half of the field grammar is exactly the keys of
//! `schema::constraints::Constraints`:
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
//!             | pattern = "<ECMA-262 regex>"      with the `pattern` feature
//!             | min_items = <integer> | max_items = <integer>
//!             | unique_items
//! ```
//!
//! `format` is deliberately absent: it follows from the type, so naming it here
//! is an error that points at the remedy.
//!
//! `open` is not a constraint: it says how a `#[serde(flatten)]` field composes
//! (bounded by `OpenMap` rather than `Flatten`; see `flatten_witnesses`).

mod aliases;
pub(crate) mod attributes;
pub(crate) mod check;
mod kinds;
#[cfg(feature = "pattern")]
mod pattern;
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

    // Only a concrete type claims a component name: `Page<User>` and
    // `Page<Order>` would collide, so a generic type is inlined (and so may not
    // recurse; see `refusals::recursion`).
    let component = LitStr::new(&name.to_string(), name.span());
    let named = if input.generics.type_params().next().is_some() {
        quote!(::core::option::Option::None)
    } else {
        quote!(::kynos::openapi::ComponentName::sanitized(#component).ok())
    };

    let container = Container::read(input);
    let body = body(input, &container);
    let check = check::body(input, &container);
    let kinds = kinds::newtype(input, &container, &generics);
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
        #kinds

        // The impl names the type, so a `#[deprecated]` type would otherwise warn
        // at its own definition (as serde's derives also allow).
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

            // A type with nothing to check reads neither argument.
            #[allow(unused_variables)]
            fn check_constraints(
                &self,
                at: ::kynos::schema::constraints::Pointer<'_>,
                violations: &mut ::kynos::schema::constraints::Violations,
            ) {
                #check
            }
        }
    })
}

/// The input's generics, with `Schema` required of each type parameter.
///
/// Parameters rather than field types, as serde does: a field-type bound would
/// demand `PhantomData<T>: Schema`, which nothing satisfies.
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
/// A `const _` rather than an impl predicate, so the diagnostic lands on the
/// type's definition. Bounds: `OpenMap` for `#[schema(open)]` (`AdmitsAny`
/// beside an unread field), else `Flatten`, plus `ClosedFlatten` in an object
/// [`closed`] closes. An internally tagged newtype variant's payload, composed
/// with a tag-only object in an `allOf`, is bounded by `Flatten` alone.
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
        // Spanned at the field's type, so the refusal points at what was written.
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
/// The schema omits the unread field, so the open field must admit any member:
/// [`flatten_witnesses`] bounds it by `AdmitsAny`. A transparent struct has no
/// object to bound.
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
/// True of an object whose `properties` names every member it admits: a named
/// struct, or an internally or adjacently tagged enum of such branches. Never
/// for an open, transparent or closed container, nor one with a field serde
/// writes but never reads, since each would constrain the outer object's
/// members one level up.
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
                // Adjacently tagged: every branch is a tag and content object.
                (Some(_), Some(_)) => !variants.is_empty() && !container.deny_unknown_fields,
                // Internally tagged: a named or unit variant is an object of its
                // members plus the tag.
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
                // Externally tagged: each branch is closed to all but its
                // variant key, and a unit variant is a bare string.
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
/// serde takes a key only through `deserialize_struct`, which it uses unless a
/// field is flattened without `skip_deserializing` (so a flattened `PhantomData`
/// counts) or the struct carries a container `tag`. An adjacently tagged enum
/// names its keys; an internally tagged one reads through `deserialize_any`.
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

/// Each group of fields the input declares: a struct's, or one per variant.
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
/// Attribute grammar still reads [`field_groups`]: a malformed attribute is an
/// error wherever it is written.
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
/// both ways. A read-only variant is among them, since serde accepts it.
fn described_variants(data: &DataEnum) -> Vec<&Variant> {
    data.variants
        .iter()
        .filter(|variant| !is_skipped_both_ways(&variant.attrs))
        .collect()
}

/// Whether serde writes a variant: it carries neither `skip` nor
/// `skip_serializing`.
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
/// in order: each one serde does not skip both ways. Not [`is_described`],
/// which would drop a `skip_deserializing` member the refusals must still see.
fn positional_members(fields: &Punctuated<Field, Comma>) -> Vec<&Field> {
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

/// What the type's own serde attributes said, read rather than restated.
#[derive(Clone, Default)]
struct Container {
    /// The container `rename`, serialize side: the name serde writes a
    /// struct's `#[serde(tag = "...")]` as.
    rename: Option<String>,
    /// The container `rename_all` style, serialize side; a split one is refused
    /// first ([`split_rule`]). In [`Container::fields_of`], a variant's.
    rename_all: Option<String>,
    /// An enum's `rename_all_fields`: names a variant's fields where the
    /// variant has no `rename_all` of its own.
    rename_all_fields: Option<String>,
    tag: Option<String>,
    content: Option<String>,
    /// `#[serde(transparent)]`: the wire form is the one field's value.
    transparent: bool,
    doc: Option<String>,
    /// A container `#[serde(default)]` on a non-unit struct.
    default: bool,
    /// `#[serde(deny_unknown_fields)]`, which [`closed`] follows.
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
            // Shape errors in serde's own attribute are serde's to report.
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

    /// The container a variant's fields are named under: the variant's own
    /// `rename_all`, else the enum's `rename_all_fields`, in place of the enum's
    /// `rename_all`, which serde applies to variant names alone. Serialize side
    /// for a written variant, deserialize side for a read-only one.
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
/// A boolean schema carries no keyword; a `$ref` does, since 3.1 applies its
/// siblings. Never `Some(false)`, the specification's default.
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
/// `additionalProperties: false`, or `unevaluatedProperties: false` where the
/// object carries an `allOf`, whose members `additionalProperties` cannot see.
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
/// Shared with [`multipart`](super::multipart) so part and property names agree.
pub(super) fn property_names(input: &DeriveInput, fields: &syn::FieldsNamed) -> Vec<String> {
    let container = Container::read(input);
    fields
        .named
        .iter()
        .map(|field| field_name(field, &container))
        .collect()
}

/// The span of the first field whose split `rename` gives serde's two
/// directions different names, at that `rename`; [`multipart`](super::multipart)
/// refuses it, since a part has one name.
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
/// whose sides differ, one side left out included, at that `key`. Shape errors
/// stay serde's to report.
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
