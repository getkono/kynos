//! `#[derive(HeaderParams)]`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input, spanned::Spanned};

use crate::derive::common::{named_fields, names_const, reject_duplicate_names, wire_name};

/// Fields a header parameter definition may not name, and what to reach for.
///
/// The specification says a parameter definition for the first three *shall be
/// ignored*, so declaring one is a claim no consumer will honour.
/// `Content-Type` on a response is likewise derived from the content map.
const RESERVED: &[(&str, &str)] = &[
    (
        "accept",
        "content negotiation decides this; return `Negotiated<T>` and let the description say \
         which representations exist",
    ),
    (
        "content-type",
        "the content map decides this; it follows from the body type rather than being declared \
         beside it",
    ),
    (
        "authorization",
        "declare the credential instead, with `#[derive(SecurityScheme)]`, so that enforcing it \
         and describing it are one act",
    ),
];

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let fields = named_fields(input, "Headers")?;

    let mut names = Vec::with_capacity(fields.named.len());
    for field in &fields.named {
        let name = wire_name(field, "header")?;
        // HTTP field names are case-insensitive, so the check must be too.
        let folded = name.to_ascii_lowercase();
        if let Some((_, remedy)) = RESERVED.iter().find(|(reserved, _)| *reserved == folded) {
            return Err(syn::Error::new(
                field.span(),
                format!("`{name}` must not be declared as a header parameter: {remedy}"),
            ));
        }
        names.push(name);
    }
    reject_duplicate_names(fields, &names, "header")?;

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let names = names_const(&names);

    Ok(quote! {
        impl #impl_generics ::kynos::extract::params::header::HeaderParams
            for #name #ty_generics #where_clause
        {
            #names

            fn decode(
                headers: &::kynos::http::HeaderMap,
            ) -> ::core::result::Result<Self, ::kynos::error::rejection::HeaderRejection> {
                let _ = headers;
                ::core::todo!()
            }

            fn encode(
                &self,
            ) -> ::std::vec::Vec<(::kynos::http::HeaderName, ::kynos::http::HeaderValue)> {
                ::core::todo!()
            }

            fn parameters(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::std::vec::Vec<::kynos::openapi::Parameter> {
                let _ = registry;
                ::core::todo!()
            }

            fn response_headers(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::kynos::openapi::Map<::kynos::openapi::RefOr<::kynos::openapi::Header>> {
                let _ = registry;
                ::core::todo!()
            }
        }
    })
}
