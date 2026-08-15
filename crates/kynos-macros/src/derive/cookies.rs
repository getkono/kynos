//! `#[derive(CookieParams)]`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

use crate::derive::{
    common::{named_fields, names_const, reject_duplicate_names, wire_name},
    params::{Param, construct, decode_field, parameters_body},
};

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let fields = named_fields(input, "Cookies")?;
    let names = fields
        .named
        .iter()
        .map(|field| wire_name(field, "cookie"))
        .collect::<syn::Result<Vec<_>>>()?;
    reject_duplicate_names(fields, &names, "cookie")?;

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let names_item = names_const(&names);

    let params = Param::pair(fields, &names);
    let rejection = quote!(::kynos::error::rejection::CookieRejection);

    // A request may carry several `Cookie` fields and each may hold several
    // pairs, so the jar is the concatenation of both. A value the client wrote
    // in RFC 6265's quoted form is unwrapped, since the quotes delimit the
    // value rather than belonging to it.
    let jar = quote! {
        let mut jar: ::std::vec::Vec<(&str, &str)> = ::std::vec::Vec::new();
        for field in headers.get_all(::kynos::http::header::COOKIE) {
            let ::core::result::Result::Ok(text) = field.to_str() else {
                continue;
            };
            for entry in text.split(';') {
                let entry = entry.trim();
                if entry.is_empty() {
                    continue;
                }
                let (name, value) = match entry.split_once('=') {
                    ::core::option::Option::Some(split) => split,
                    ::core::option::Option::None => (entry, ""),
                };
                let value = value.trim();
                let value = value
                    .strip_prefix('"')
                    .and_then(|value| value.strip_suffix('"'))
                    .unwrap_or(value);
                jar.push((name.trim(), value));
            }
        }
    };

    let reads = params.iter().map(|param| {
        let wire = param.name();
        let found = quote! {
            jar.iter().find_map(|&(name, value)| (name == #wire).then_some(value))
        };
        decode_field(param, &rejection, &found, "the cookie is required")
    });
    let value = construct(&params);

    let parameters = parameters_body(
        &params,
        &quote!(::kynos::openapi::ParameterIn::Cookie),
        false,
    );

    Ok(quote! {
        impl #impl_generics ::kynos::extract::params::cookie::CookieParams
            for #name #ty_generics #where_clause
        {
            #names_item

            fn decode(
                headers: &::kynos::http::HeaderMap,
            ) -> ::core::result::Result<Self, ::kynos::error::rejection::CookieRejection> {
                #jar
                #(#reads)*
                #value
            }

            fn parameters(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::std::vec::Vec<::kynos::openapi::Parameter> {
                #parameters
            }
        }
    })
}
