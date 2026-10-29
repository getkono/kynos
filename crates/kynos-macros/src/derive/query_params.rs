//! `#[derive(QueryParams)]`.

use quote::quote;
use syn::{DeriveInput, Ident};

use crate::derive::{
    common::{NameCase, named_fields, reject_duplicate_names, wire_names},
    params::{Param, check_field, construct, decode_field, parameters_body, render},
};

pub(crate) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let fields = named_fields(input, "QueryParams")?;
    let names = wire_names(input, fields, "param")?;
    reject_duplicate_names(fields, &names, "query parameter", NameCase::Sensitive)?;

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let params = Param::pair(fields, &names);
    let rejection = quote!(::kynos::error::rejection::QueryRejection);

    // The first occurrence of a repeated name wins.
    let pairs = query_slots(&params);
    let reads = params.iter().enumerate().map(|(index, param)| {
        let wire = param.name();
        let slot = query_slot(index);
        // Only a declared parameter's value is required to be UTF-8.
        let found = quote! {
            match &#slot {
                ::core::option::Option::Some(octets) => match ::core::str::from_utf8(octets) {
                    ::core::result::Result::Ok(text) => ::core::option::Option::Some(text),
                    ::core::result::Result::Err(_) => {
                        return ::core::result::Result::Err(
                            #rejection::Invalid {
                                name: <::std::string::String as ::core::convert::From<_>>::from(
                                    #wire,
                                ),
                                detail: <::std::string::String as ::core::convert::From<_>>::from(
                                    "the percent-decoded value is not UTF-8",
                                ),
                            },
                        );
                    }
                },
                ::core::option::Option::None => ::core::option::Option::None,
            }
        };
        let decoded = decode_field(param, &rejection, &found, "the parameter is required");
        let checked = check_field(param, &rejection);
        quote!(#decoded #checked)
    });
    let value = construct(&params);

    let parameters = parameters_body(
        &params,
        &quote!(::kynos::openapi::ParameterIn::Query),
        false,
    );
    let encode = encode_body(&params);

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

/// One pass over the pairs a raw query string carries, binding each declared
/// parameter's [`query_slot`] to the octets of the first pair that names it.
///
/// A name is compared as octets once decoded, and only a declared parameter's
/// value is decoded at all, so a pair no field reads costs its name's decoding
/// alone and nothing is collected. Split by `kynos::__private::uri::query_names`,
/// which shares its decoding with query API keys.
fn query_slots(params: &[Param<'_>]) -> proc_macro2::TokenStream {
    if params.is_empty() {
        return quote!(let _ = query;);
    }

    let slots: Vec<Ident> = (0..params.len()).map(query_slot).collect();
    let arms = params.iter().zip(&slots).map(|(param, slot)| {
        let wire = proc_macro2::Literal::byte_string(param.name().as_bytes());
        quote! {
            #wire => {
                if #slot.is_none() {
                    #slot = ::core::option::Option::Some(
                        ::kynos::__private::uri::decode_query_component(value),
                    );
                }
            }
        }
    });

    quote! {
        #(
            let mut #slots: ::core::option::Option<::std::borrow::Cow<'_, [u8]>> =
                ::core::option::Option::None;
        )*
        for (name, value) in ::kynos::__private::uri::query_names(query) {
            match &*name {
                #(#arms)*
                _ => {}
            }
        }
    }
}

/// The local [`query_slots`] binds the `index`th declared parameter's octets
/// to.
fn query_slot(index: usize) -> Ident {
    quote::format_ident!("__kynos_query_{index}")
}

/// The `EncodeQuery::encode` body.
///
/// An absent optional parameter is omitted rather than written empty. Each
/// name and value is encoded by `kynos::__private::uri::encode_query_component`,
/// so it round-trips through the decoder [`query_slots`] writes.
fn encode_body(params: &[Param<'_>]) -> proc_macro2::TokenStream {
    let entries = params.iter().map(|param| {
        let name = param.name();
        let rendered = render(param);
        quote! {
            if let ::core::option::Option::Some(value) = #rendered {
                if !query.is_empty() {
                    query.push('&');
                }
                query.push_str(&::kynos::__private::uri::encode_query_component(#name));
                query.push('=');
                query.push_str(&::kynos::__private::uri::encode_query_component(&value));
            }
        }
    });

    quote! {
        let mut query = ::std::string::String::new();
        #(#entries)*
        query
    }
}
