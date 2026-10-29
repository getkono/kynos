//! `#[derive(Schema)]`.
//!
//! The constraint half of the field grammar is exactly the keys of
//! `schema::constraints::Constraints`:
//!
//! ```text
//! #[schema( <member> [, <member>]* )]             on a field, optional
//!
//! member := <constraint> | open | as = <type>
//!
//! constraint := minimum = <number> | maximum = <number>
//!             | exclusive_minimum = <number> | exclusive_maximum = <number>
//!             | multiple_of = <number>
//!             | min_length = <integer> | max_length = <integer>
//!             | pattern = "<ECMA-262 regex>"      with the `pattern` feature
//!             | min_items = <integer> | max_items = <integer>
//!             | unique_items
//!             | content_encoding = "<RFC 4648 or RFC 2045 encoding>"
//!             | content_media_type = "<RFC 9110 media type>"
//! ```
//!
//! The two `content_*` keys are annotations, as JSON Schema makes them: the
//! check holds the field to a string and asserts nothing about its content.
//!
//! `format` is deliberately absent: it follows from the type, so naming it here
//! is an error that points at the remedy.
//!
//! `open` is not a constraint: it says how a `#[serde(flatten)]` field composes
//! (bounded by `OpenMap` rather than `Flatten`; see `flatten::witnesses`).
//!
//! Nor is `as`: it resolves the field's schema from another type, and its
//! check runs on that type's value, converted through `From<&F>`.

pub(crate) mod aliases;
pub(crate) mod attributes;
pub(crate) mod check;
mod container;
mod flatten;
mod kinds;
pub(crate) mod members;
#[cfg(feature = "pattern")]
mod pattern;
pub(crate) mod refusals;
mod shape;

use attributes::{
    constraints, field_read_name, is_described, is_flattened, is_open, is_phantom, is_required,
    is_unit_like, transparent_member, variant_read_name,
};
use container::Container;
use members::{described_variants, positional_members};
use shape::{enum_body, struct_body};

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    Data, DataEnum, DeriveInput, Field, Fields, Lit, LitFloat, LitInt, LitStr, Type, Variant,
    ext::IdentExt, punctuated::Punctuated, spanned::Spanned, token::Comma,
};

use crate::derive::common::skip_value;
use crate::doc::{doc_string, is_deprecated};

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

/// Keys taking a string JSON Schema reads as an annotation: described on a
/// string field and asserted by nothing.
const ANNOTATIONS: &[&str] = &["content_encoding", "content_media_type"];

pub(crate) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
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

    // A generic type mangles each instantiation's name from its arguments',
    // `Page_User`, and is inlined where one has none, which the derive cannot
    // see (so it may not recurse; see `refusals::recursion`). A const argument
    // names itself by its value, since `Batch<User, 2>` and `Batch<User, 3>`
    // describe different arrays.
    let component = LitStr::new(&name.to_string(), name.span());
    let arguments: Vec<TokenStream2> = input
        .generics
        .params
        .iter()
        .filter_map(|parameter| match parameter {
            syn::GenericParam::Type(parameter) => {
                let ident = &parameter.ident;
                Some(quote!(<#ident as ::kynos::schema::Schema>::name()))
            }
            syn::GenericParam::Const(parameter) => {
                let ident = &parameter.ident;
                Some(quote! {
                    ::kynos::openapi::ComponentName::sanitized(
                        &::std::string::ToString::to_string(&#ident),
                    )
                    .ok()
                })
            }
            syn::GenericParam::Lifetime(_) => None,
        })
        .collect();
    let named = if arguments.is_empty() {
        quote!(::kynos::openapi::ComponentName::sanitized(#component).ok())
    } else {
        quote! {
            ::kynos::schema::instantiation_name(
                #component,
                &[#(#arguments),*],
            )
        }
    };

    let container = Container::read(input);
    let body = body(input, &container);
    let check = check::body(input, &container);
    let kinds = kinds::newtype(input, &container, &generics);
    let flatten = flatten::emit(input, &container, &generics);

    Ok(quote! {
        #flatten
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
        Data::Union(_) => quote!(<::kynos::openapi::Schema as ::core::default::Default>::default()),
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
                    ::core::option::Option::Some(
                        <::std::string::String as ::core::convert::From<_>>::from(#doc),
                    );
            }
            described
        }
    }
}
