//! `#[derive(QueryParams)]`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

use crate::derive::{
    common::{named_fields, reject_duplicate_names, wire_name},
    params::{Param, construct, decode_field, parameters_body, query_encode_body, query_pairs},
};

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

    let params = Param::pair(fields, &names);
    let rejection = quote!(::kynos::error::rejection::QueryRejection);

    let pairs = query_pairs();
    // The first occurrence wins. A repeated name is how a list is spelled in a
    // query string, and a group of named parameters is not the shape that
    // describes one -- 3.2's whole-query parameter is.
    let reads = params.iter().map(|param| {
        let wire = param.name();
        let found = quote! {
            pairs
                .iter()
                .find_map(|(name, value)| (name.as_str() == #wire).then_some(value.as_str()))
        };
        decode_field(param, &rejection, &found, "the parameter is required")
    });
    let value = construct(&params);

    let parameters = parameters_body(
        &params,
        &quote!(::kynos::openapi::ParameterIn::Query),
        false,
    );
    let encode = query_encode_body(&params);

    Ok(quote! {
        impl #impl_generics ::kynos::extract::params::query::QueryParams
            for #name #ty_generics #where_clause
        {
            fn decode(
                query: ::core::option::Option<&str>,
            ) -> ::core::result::Result<Self, ::kynos::error::rejection::QueryRejection> {
                #pairs
                #(#reads)*
                #value
            }

            fn encode(&self) -> ::std::string::String {
                #encode
            }

            fn parameters(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::std::vec::Vec<::kynos::openapi::Parameter> {
                #parameters
            }
        }
    })
}
