//! `#[derive(PathParams)]`.

use quote::quote;
use syn::{DeriveInput, spanned::Spanned};

use crate::derive::{
    common::{NameCase, named_fields, names_const, reject_duplicate_names, wire_names},
    params::{Param, check_field, construct, decode_field, parameters_body, path_encode_body},
};

/// A path variable is always captured, and an absent one would render as an
/// empty segment no route matches, so a path parameter cannot be optional.
fn reject_optional(params: &[Param<'_>]) -> syn::Result<()> {
    match params.iter().find(|param| param.optional().is_some()) {
        Some(param) => Err(syn::Error::new(
            param.ty().span(),
            "a path parameter is always present, so it cannot be an `Option`. Use the inner \
             type, or move the optional value to a `QueryParams` group",
        )),
        None => Ok(()),
    }
}

pub(crate) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let fields = named_fields(input, "PathParams")?;
    let names = wire_names(input, fields, "param")?;
    reject_duplicate_names(fields, &names, "path parameter", NameCase::Sensitive)?;

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let names_item = names_const(&names);

    let params = Param::pair(fields, &names);
    reject_optional(&params)?;
    let rejection = quote!(::kynos::error::rejection::PathRejection);

    // Looked up by name, so decoding does not depend on the route attribute's
    // order assertion.
    let reads = params.iter().map(|param| {
        let wire = param.name();
        let found = quote! {
            ::core::iter::Iterator::find_map(&mut values.iter(),|&(name, value)| {
                (name == #wire).then_some(value)
            })
        };
        let decoded = decode_field(
            param,
            &rejection,
            &found,
            "the path variable was not captured",
        );
        let checked = check_field(param, &rejection);
        quote!(#decoded #checked)
    });
    let value = construct(&params);

    let parameters = parameters_body(&params, &quote!(::kynos::openapi::ParameterIn::Path), true);
    let encode = path_encode_body(&params);

    // Three implementations, one per direction plus the description.
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
