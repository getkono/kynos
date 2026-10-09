//! `#[derive(QueryParams)]`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

use crate::derive::{
    common::{NameCase, named_fields, reject_duplicate_names, wire_names},
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
    let names = wire_names(input, fields, "param")?;
    reject_duplicate_names(fields, &names, "query parameter", NameCase::Sensitive)?;

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
        // A name that is not UTF-8 never equals a declared one, so only a
        // declared value is held to being text.
        let found = quote! {
            match pairs.iter().find_map(|(name, value)| {
                (**name == *#wire.as_bytes()).then_some(&**value)
            }) {
                ::core::option::Option::Some(octets) => match ::core::str::from_utf8(octets) {
                    ::core::result::Result::Ok(text) => ::core::option::Option::Some(text),
                    ::core::result::Result::Err(_) => {
                        return ::core::result::Result::Err(
                            #rejection::Invalid {
                                name: ::std::string::String::from(#wire),
                                detail: ::std::string::String::from(
                                    "the percent-decoded value is not UTF-8",
                                ),
                            },
                        );
                    }
                },
                ::core::option::Option::None => ::core::option::Option::None,
            }
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

    // Three implementations; see `path_params.rs` for why the directions are
    // separate traits.
    Ok(quote! {
        impl #impl_generics ::kynos::extract::params::query::QueryParams
            for #name #ty_generics #where_clause
        {
            fn parameters(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::std::vec::Vec<::kynos::openapi::Parameter> {
                #parameters
            }
        }

        impl #impl_generics ::kynos::extract::params::query::DecodeQuery
            for #name #ty_generics #where_clause
        {
            fn decode(
                query: ::core::option::Option<&str>,
            ) -> ::core::result::Result<Self, ::kynos::error::rejection::QueryRejection> {
                #pairs
                #(#reads)*
                #value
            }
        }

        impl #impl_generics ::kynos::extract::params::query::EncodeQuery
            for #name #ty_generics #where_clause
        {
            fn encode(&self) -> ::std::string::String {
                #encode
            }
        }
    })
}
