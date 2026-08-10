//! `#[derive(QueryParams)]`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

use crate::derive::common::{named_fields, reject_duplicate_names, wire_name};

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let fields = named_fields(input, "QueryParams")?;
    let names = fields
        .named
        .iter()
        .map(|field| wire_name(field, "param"))
        .collect::<syn::Result<Vec<_>>>()?;
    reject_duplicate_names(fields, &names, "query parameter")?;

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics ::kynos::extract::params::query::QueryParams
            for #name #ty_generics #where_clause
        {
            fn decode(
                query: ::core::option::Option<&str>,
            ) -> ::core::result::Result<Self, ::kynos::error::rejection::QueryRejection> {
                let _ = query;
                ::core::todo!()
            }

            fn encode(&self) -> ::std::string::String {
                ::core::todo!()
            }

            fn parameters(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::std::vec::Vec<::kynos::openapi::Parameter> {
                let _ = registry;
                ::core::todo!()
            }
        }
    })
}
