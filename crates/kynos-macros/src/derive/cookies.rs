//! `#[derive(CookieParams)]`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

use crate::derive::{
    common::{named_fields, names_const, reject_duplicate_names, wire_names},
    params::{Param, construct, decode_field, parameters_body},
};

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

pub(super) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let fields = named_fields(input, "Cookies")?;
    let names = wire_names(input, fields, "cookie")?;
    reject_duplicate_names(fields, &names, "cookie")?;

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let names_item = names_const(&names);

    let params = Param::pair(fields, &names);
    let rejection = quote!(::kynos::error::rejection::CookieRejection);

    // Splitting a jar is `http::cookie`'s job, not an expansion's: the rules
    // are RFC 6265's, they belong in one place, and a credential carried in a
    // cookie reads them from there in a build with no `cookie` feature at all.
    // A declared cookie that was sent but cannot be read is refused rather
    // than reported missing: it was not absent. The detail is a literal, as
    // the query and header derives write theirs, so a 400's wording does not
    // move with `Unreadable`'s `Display`.
    let reads = params.iter().map(|param| {
        let wire = param.name();
        let found = quote! {
            match ::kynos::http::cookie::value_of(headers, #wire) {
                ::core::result::Result::Ok(found) => found,
                ::core::result::Result::Err(_) => {
                    return ::core::result::Result::Err(#rejection::Invalid {
                        name: ::std::string::String::from(#wire),
                        detail: ::std::string::String::from("the cookie value is not ASCII"),
                    });
                }
            }
        };
        decode_field(param, &rejection, &found, "the cookie is required")
    });
    let value = construct(&params);

    let parameters = parameters_body(
        &params,
        &quote!(::kynos::openapi::ParameterIn::Cookie),
        false,
    );
    // A cookie reaches its field as it was sent: `value_of` removes no
    // percent-encoding. An unstated style defaults to `form`, which tells a
    // client to percent-encode, so a 3.2 build states `cookie`, which applies
    // and removes none; `explode: true` is its default, and a scalar field
    // ignores it. 3.1 has no style saying so, and a description stating this
    // one cannot be emitted as 3.1, so a 3.1 build states nothing.
    let parameters = if cfg!(feature = "openapi32") {
        quote! {
            let parameters = { #parameters };
            parameters
                .into_iter()
                .map(|parameter| {
                    parameter.with_style(::kynos::openapi::Style::Cookie, true)
                })
                .collect()
        }
    } else {
        parameters
    };

    Ok(quote! {
        impl #impl_generics ::kynos::extract::params::cookie::CookieParams
            for #name #ty_generics #where_clause
        {
            #names_item

            fn decode(
                headers: &::kynos::http::HeaderMap,
            ) -> ::core::result::Result<Self, ::kynos::error::rejection::CookieRejection> {
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
