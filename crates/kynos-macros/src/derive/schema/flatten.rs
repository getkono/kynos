//! What `#[serde(flatten)]` asks of a type: the witnesses bounding each field
//! it flattens, and whether the type may itself be flattened.

use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use syn::{Data, DeriveInput, Field, Fields, spanned::Spanned};

use super::{
    Container,
    attributes::{described_members, is_flattened, is_open, is_unit_like, serde_flag},
    described_groups, described_variants, is_written, unread_field_span, written_groups,
};

/// The flatten witnesses, then the `Flatten` and `ClosedFlatten` claims the
/// input's own shape earns.
pub(super) fn emit(
    input: &DeriveInput,
    container: &Container,
    generics: &syn::Generics,
) -> TokenStream2 {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let witnesses = witnesses(input, container, generics);
    let flatten = flattens(input, container).then(|| {
        quote! {
            #[allow(deprecated)]
            impl #impl_generics ::kynos::schema::flatten::Flatten
                for #name #ty_generics #where_clause {}
        }
    });
    let closed_flatten = flattens_into_closed_objects(input, container).then(|| {
        quote! {
            #[allow(deprecated)]
            impl #impl_generics ::kynos::schema::flatten::ClosedFlatten
                for #name #ty_generics #where_clause {}
        }
    });

    quote! {
        #witnesses
        #flatten
        #closed_flatten
    }
}

/// One witness per flattened field, requiring that its type names its members.
///
/// A `const _` rather than an impl predicate, so the diagnostic lands on the
/// type's definition. Bounds: `OpenMap` for `#[schema(open)]` (`AdmitsAny`
/// beside an unread field), else `Flatten`, plus `ClosedFlatten` in an object
/// [`closed`](super::closed) closes. An internally tagged newtype variant's
/// payload, composed with a tag-only object in an `allOf`, is bounded by
/// `Flatten` alone.
fn witnesses(input: &DeriveInput, container: &Container, generics: &syn::Generics) -> TokenStream2 {
    let (impl_generics, _, where_clause) = generics.split_for_impl();

    let closing = container.deny_unknown_fields && !container.transparent;
    let flattened = described_groups(input, container)
        .into_iter()
        .flat_map(described_members)
        .filter(|field| is_flattened(field))
        .map(|field| (field, closing));

    let admitting = open_fields_beside_unread_fields(input, container);

    let payloads = internally_tagged_payloads(input, container)
        .into_iter()
        .map(|field| (field, false));
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

/// The payload of each described newtype variant of an internally tagged enum:
/// serde writes its members beside the tag, as it does a flattened field's.
pub(super) fn internally_tagged_payloads<'a>(
    input: &'a DeriveInput,
    container: &Container,
) -> Vec<&'a Field> {
    match (&input.data, &container.tag, &container.content) {
        (Data::Enum(data), Some(_), None) => described_variants(data, container)
            .into_iter()
            .filter(|variant| !is_unit_like(&variant.fields))
            .filter_map(|variant| match &variant.fields {
                Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => unnamed.unnamed.first(),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The open flattened fields that sit beside a named field serde writes and
/// never reads, `skip_deserializing` alone, in an object serde writes.
///
/// The schema omits the unread field, so the open field must admit any member:
/// [`witnesses`] bounds it by `AdmitsAny`. A transparent struct has no
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

    let groups = described_groups(input, container);
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
            let variants = described_variants(data, container);

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
