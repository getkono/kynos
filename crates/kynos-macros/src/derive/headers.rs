//! `#[derive(HeaderParams)]`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input, spanned::Spanned};

use crate::derive::{
    common::{named_fields, names_const, reject_duplicate_names, wire_name},
    params::{
        Param, construct, decode_field, header_encode_body, parameters_body, response_headers_body,
    },
};

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

pub(super) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
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
    let names_item = names_const(&names);

    let params = Param::pair(fields, &names);
    let rejection = quote!(::kynos::error::rejection::HeaderRejection);

    // Only the first value of a repeated field is read. A field this group
    // declares is one typed value; the repeated ones -- `Set-Cookie` above all
    // -- are what `encode` keeps separate, and no OpenAPI parameter describes
    // a group of them.
    let reads = params.iter().map(|param| {
        let wire = param.name();
        let found = quote! {
            match headers.get(#wire) {
                ::core::option::Option::Some(value) => match value.to_str() {
                    ::core::result::Result::Ok(text) => ::core::option::Option::Some(text),
                    ::core::result::Result::Err(_) => {
                        return ::core::result::Result::Err(
                            ::kynos::error::rejection::HeaderRejection::Invalid {
                                name: ::std::string::String::from(#wire),
                                detail: ::std::string::String::from(
                                    "the value is not printable ASCII",
                                ),
                            },
                        );
                    }
                },
                ::core::option::Option::None => ::core::option::Option::None,
            }
        };
        decode_field(param, &rejection, &found, "the header is required")
    });
    let value = construct(&params);

    let parameters = parameters_body(
        &params,
        &quote!(::kynos::openapi::ParameterIn::Header),
        false,
    );
    let response_headers = response_headers_body(&params);
    let encode = header_encode_body(&params);

    // Three implementations; see `path_params.rs` for why the directions are
    // separate traits. A derived group does both, which is why `Headers<T>`
    // and `WithHeaders<_, T>` both keep working on one derive.
    Ok(quote! {
        impl #impl_generics ::kynos::extract::params::header::DecodeHeaders
            for #name #ty_generics #where_clause
        {
            fn decode(
                headers: &::kynos::http::HeaderMap,
            ) -> ::core::result::Result<Self, ::kynos::error::rejection::HeaderRejection> {
                #(#reads)*
                #value
            }
        }

        impl #impl_generics ::kynos::extract::params::header::EncodeHeaders
            for #name #ty_generics #where_clause
        {
            fn encode(
                &self,
            ) -> ::std::vec::Vec<(::kynos::http::HeaderName, ::kynos::http::HeaderValue)> {
                #encode
            }
        }

        impl #impl_generics ::kynos::extract::params::header::HeaderParams
            for #name #ty_generics #where_clause
        {
            #names_item

            fn parameters(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::std::vec::Vec<::kynos::openapi::Parameter> {
                #parameters
            }

            fn response_headers(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::kynos::openapi::Map<::kynos::openapi::RefOr<::kynos::openapi::Header>> {
                #response_headers
            }
        }
    })
}
