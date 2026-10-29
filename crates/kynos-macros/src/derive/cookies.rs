//! `#[derive(CookieParams)]`.

use quote::quote;
use syn::{DeriveInput, Field};

use crate::derive::{
    attribute::field_args,
    common::{
        NameCase, named_fields, names_const, reject_duplicate_names, reject_non_token_names,
        wire_names,
    },
    params::{Param, check_field, construct, decode_field, parameters_body},
};

pub(crate) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let fields = named_fields(input, "Cookies")?;
    let names = wire_names(input, fields, "cookie")?;
    reject_non_token_names(fields, &names, "cookie", "an RFC 6265 cookie name")?;
    // Cookie names are matched exactly, as `http::cookie::value_of` reads them.
    reject_duplicate_names(fields, &names, "cookie", NameCase::Sensitive)?;

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let names_item = names_const(&names);

    let params = Param::pair(fields, &names);
    let rejection = quote!(::kynos::error::rejection::CookieRejection);

    // RFC 6265 jar splitting stays in `http::cookie`. A sent but unreadable
    // cookie is invalid, not missing; the detail is a literal so it is stable.
    let reads = params.iter().map(|param| {
        let wire = param.name();
        let found = quote! {
            match ::kynos::http::cookie::value_of(headers, #wire) {
                ::core::result::Result::Ok(found) => found,
                ::core::result::Result::Err(_) => {
                    return ::core::result::Result::Err(#rejection::Invalid {
                        name: <::std::string::String as ::core::convert::From<_>>::from(#wire),
                        detail: <::std::string::String as ::core::convert::From<_>>::from(
                            "the cookie value is not ASCII",
                        ),
                    });
                }
            }
        };
        let decoded = decode_field(param, &rejection, &found, "the cookie is required");
        let checked = check_field(param, &rejection);
        quote!(#decoded #checked)
    });
    let value = construct(&params);

    let parameters = parameters_body(
        &params,
        &quote!(::kynos::openapi::ParameterIn::Cookie),
        false,
    );
    // `value_of` removes no percent-encoding, which OpenAPI 3.2's `cookie` style
    // states and the default `form` does not. Stating it is opt-in, since it
    // raises the description to 3.2 and the `openapi32` feature must not.
    let styled = fields
        .named
        .iter()
        .map(cookie_style)
        .collect::<syn::Result<Vec<bool>>>()?;
    let parameters = if styled.contains(&true) {
        quote! {
            let parameters: ::std::vec::Vec<::kynos::openapi::Parameter> = { #parameters };
            ::core::iter::Iterator::collect(::core::iter::Iterator::map(
                ::core::iter::Iterator::zip(
                    ::core::iter::IntoIterator::into_iter(parameters),
                    [#(#styled),*],
                ),
                |(parameter, styled)| {
                    if styled {
                        parameter.with_style(::kynos::openapi::Style::Cookie, true)
                    } else {
                        parameter
                    }
                },
            ))
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

/// Whether the field states `#[cookie(style = "cookie")]`, the one style this
/// derive reads values as; a 3.1 build has no such style, so it is refused there.
fn cookie_style(field: &Field) -> syn::Result<bool> {
    let Some(literal) = field_args(field, "cookie")?.style else {
        return Ok(false);
    };
    if literal.value() != "cookie" {
        return Err(syn::Error::new(
            literal.span(),
            "a derived cookie is read as sent, with no percent-encoding removed, so the one \
             style it can state is `style = \"cookie\"`",
        ));
    }
    if cfg!(not(feature = "openapi32")) {
        return Err(syn::Error::new(
            literal.span(),
            "`style = \"cookie\"` was introduced in OpenAPI 3.2, and this build describes 3.1; \
             enable the `openapi32` feature, or drop it",
        ));
    }
    Ok(true)
}
