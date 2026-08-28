//! `#[derive(PathParams)]`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

use crate::derive::{
    common::{named_fields, names_const, reject_duplicate_names, wire_name},
    params::{Param, construct, decode_field, parameters_body, path_encode_body},
};

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let fields = named_fields(input, "PathParams")?;
    let names = fields
        .named
        .iter()
        .map(|field| wire_name(field, "param"))
        .collect::<syn::Result<Vec<_>>>()?;
    reject_duplicate_names(fields, &names, "path parameter")?;

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let names_item = names_const(&names);

    let params = Param::pair(fields, &names);
    let rejection = quote!(::kynos::error::rejection::PathRejection);

    // A capture the route matched is looked up by name rather than by
    // position, because `NAMES` is compared against the template rather than
    // assumed to be in its order.
    let reads = params.iter().map(|param| {
        let wire = param.name();
        let found = quote! {
            values
                .iter()
                .find_map(|&(name, value)| (name == #wire).then_some(value))
        };
        decode_field(
            param,
            &rejection,
            &found,
            "the path variable was not captured",
        )
    });
    let value = construct(&params);

    let parameters = parameters_body(&params, &quote!(::kynos::openapi::ParameterIn::Path), true);
    let encode = path_encode_body(&params);

    // Three implementations, because the two directions are their own traits.
    // A derive supplies both, so nothing a user writes changes; what the split
    // removes is the *hand-written* group that supplied neither and panicked.
    Ok(quote! {
        impl #impl_generics ::kynos::extract::params::path::PathParams
            for #name #ty_generics #where_clause
        {
            #names_item

            fn parameters(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::std::vec::Vec<::kynos::openapi::Parameter> {
                #parameters
            }
        }

        impl #impl_generics ::kynos::extract::params::path::DecodePath
            for #name #ty_generics #where_clause
        {
            fn decode(
                values: &[(&str, &str)],
            ) -> ::core::result::Result<Self, ::kynos::error::rejection::PathRejection> {
                #(#reads)*
                #value
            }
        }

        impl #impl_generics ::kynos::extract::params::path::EncodePath
            for #name #ty_generics #where_clause
        {
            fn encode(&self) -> ::std::vec::Vec<(&'static str, ::std::string::String)> {
                #encode
            }
        }
    })
}
