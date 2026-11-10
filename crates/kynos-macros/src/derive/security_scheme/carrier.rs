//! The derived `Carries`, and the `SecurityScheme::reads` that declares what
//! it reads beside the credential.

use quote::quote;
use syn::{DeriveInput, Type};

use super::{
    args::{Carrier, KeyLocation, Kind, Nested, SchemeArgs},
    text,
};

/// The `Carries` implementation the kind implies, or none under
/// `carrier = manual`.
pub(super) fn carrier(
    args: &SchemeArgs,
    kind: Kind,
    input: &DeriveInput,
) -> Option<proc_macro2::TokenStream> {
    if args.carrier == Some(Carrier::Manual) {
        return None;
    }

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let (mut presented, mut read) = carrier_of(&args.nested, kind);
    if let Some(reads) = &args.reads {
        (presented, read) = alongside(&presented, &read, reads);
    }
    // A malformed credential stays an `Err`; only absence becomes a value.
    if args.carrier == Some(Carrier::Optional) {
        presented = quote!(::core::option::Option<#presented>);
        read = quote!(#read.map(::core::option::Option::Some));
    }

    Some(quote! {
        impl #impl_generics ::kynos::security::carrier::Carries
            for #name #ty_generics #where_clause
        {
            type Presented = #presented;

            fn present(
                parts: &::kynos::http::Parts,
            ) -> ::core::result::Result<
                ::core::option::Option<Self::Presented>,
                ::kynos::error::rejection::AuthRejection,
            > {
                #read
            }
        }
    })
}

/// Where this kind's credential travels, and the type it reads back as; read
/// from the same `Nested` as `of_kind`.
fn carrier_of(nested: &Nested, kind: Kind) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let carrier = quote!(::kynos::security::carrier);

    match kind {
        Kind::Basic => (
            quote!(#carrier::Credentials),
            quote!(#carrier::basic(parts)),
        ),

        Kind::Http => {
            let scheme = text(nested.scheme.as_ref());
            (
                quote!(#carrier::SchemeCredentials),
                quote!(#carrier::http_scheme(parts, #scheme)),
            )
        }

        Kind::ApiKey(location) => {
            let field = text(nested.field.as_ref());
            let location = match location {
                KeyLocation::Header => quote!(#carrier::KeyLocation::Header),
                KeyLocation::Query => quote!(#carrier::KeyLocation::Query),
                KeyLocation::Cookie => quote!(#carrier::KeyLocation::Cookie),
            };
            (
                quote!(#carrier::ApiKey),
                quote!(#carrier::api_key(parts, #location, #field)),
            )
        }

        Kind::MutualTls => (
            quote!(#carrier::PeerCertificates),
            quote!(#carrier::peer_certificates(parts)),
        ),

        // An RFC 6750 bearer token.
        Kind::Bearer | Kind::OAuth2 | Kind::OpenIdConnect => (
            quote!(#carrier::BearerToken),
            quote!(#carrier::bearer(parts)),
        ),
    }
}

/// `SecurityScheme::reads`, declaring the header group `reads`.
pub(super) fn declare_reads(reads: &Type) -> proc_macro2::TokenStream {
    let header_params = quote!(::kynos::extract::params::header::HeaderParams);
    quote! {
        fn reads(
            registry: &mut ::kynos::schema::registry::Registry,
        ) -> ::std::vec::Vec<::kynos::openapi::Parameter> {
            // An undescribed group declares nothing, as `Headers` does.
            if !<#reads as #header_params>::DESCRIBED {
                return ::std::vec::Vec::new();
            }
            <#reads as #header_params>::parameters(registry)
        }
    }
}

/// The kind's carrier with `reads` decoded beside its credential: the pair
/// `(credential, reads)`.
///
/// Only a presented credential reads the group, so an anonymous request is
/// never refused for lacking a header that accompanies one. A group that does
/// not decode is a malformed credential, a 401 like any other.
fn alongside(
    presented: &proc_macro2::TokenStream,
    read: &proc_macro2::TokenStream,
    reads: &Type,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let decode = quote!(<#reads as ::kynos::extract::params::header::DecodeHeaders>::decode);
    (
        quote!((#presented, #reads)),
        // Parenthesized, so `carrier = optional` can chain onto it.
        quote! {
            (match #read? {
                ::core::option::Option::None => ::core::result::Result::Ok(
                    ::core::option::Option::None,
                ),
                ::core::option::Option::Some(credential) => #decode(&parts.headers)
                    .map(|reads| ::core::option::Option::Some((credential, reads)))
                    .map_err(|_| ::kynos::error::rejection::AuthRejection::unauthenticated()),
            })
        },
    )
}
