//! `#[derive(ProtobufMessage)]`.
//!
//! No attribute and no refusal: the expansion implements Kynos's `Message` by
//! delegating to `::prost::Message`, which resolves in the deriving crate, so
//! the protobuf implementation is the application's at the application's
//! version. A type that is not a `prost::Message` fails there, naming it.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::DeriveInput;

// `syn::Result` to fit `derive::run`, though nothing here refuses.
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn expand_inner(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics ::kynos::extract::body::protobuf::Message
            for #name #ty_generics #where_clause
        {
            fn decode(
                bytes: ::kynos::__private::Bytes,
            ) -> ::core::result::Result<Self, ::kynos::extract::body::codec::DecodeError> {
                <Self as ::prost::Message>::decode(bytes).map_err(|error| {
                    ::kynos::extract::body::codec::DecodeError::Syntax(
                        ::std::string::ToString::to_string(&error),
                    )
                })
            }

            fn encode(&self) -> ::std::vec::Vec<u8> {
                <Self as ::prost::Message>::encode_to_vec(self)
            }
        }
    })
}
