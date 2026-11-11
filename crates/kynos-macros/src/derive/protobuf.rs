//! `#[derive(ProtobufMessage)]`.
//!
//! No attribute of its own and no refusal: the expansion implements Kynos's
//! `Message` by delegating to the `prost::Message` the type derives, which
//! resolves in the deriving crate, so the protobuf implementation is the
//! application's. It reaches `prost` where `prost`'s own derive does: through
//! `#[prost(prost_path = "...")]` on the type, else `::prost`. A type that is
//! not a `prost::Message` fails there, naming it.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{Attribute, DeriveInput, Expr, ExprLit, Lit, Meta, MetaNameValue, Path, Token};

// `syn::Result` to fit `derive::run`, though nothing here refuses.
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn expand_inner(input: &DeriveInput) -> syn::Result<TokenStream2> {
    // A `prost` attribute `prost`'s derive cannot read is its diagnostic to
    // raise; an impl here would only add a second error beside it.
    let Some(prost) = prost_path(&input.attrs) else {
        return Ok(TokenStream2::new());
    };
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics ::kynos::extract::body::protobuf::Message
            for #name #ty_generics #where_clause
        {
            fn decode(
                bytes: ::kynos::__private::Bytes,
            ) -> ::core::result::Result<Self, ::kynos::extract::body::codec::DecodeError> {
                <Self as #prost::Message>::decode(bytes).map_err(|error| {
                    ::kynos::extract::body::codec::DecodeError::Syntax(
                        ::std::string::ToString::to_string(&error),
                    )
                })
            }

            fn encode(&self) -> ::std::vec::Vec<u8> {
                <Self as #prost::Message>::encode_to_vec(self)
            }
        }
    })
}

/// The path `prost`'s derive names `prost` by, read as it reads it: a
/// `prost_path = "..."` among the type's `#[prost(...)]` arguments, else
/// `::prost`. `None` where an argument list or the path does not parse.
fn prost_path(attrs: &[Attribute]) -> Option<Path> {
    let mut found = None;
    for attr in attrs {
        let Meta::List(list) = &attr.meta else {
            continue;
        };
        if !list.path.is_ident("prost") {
            continue;
        }
        let arguments = list
            .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            .ok()?;
        for argument in arguments {
            if let Meta::NameValue(MetaNameValue {
                path,
                value:
                    Expr::Lit(ExprLit {
                        lit: Lit::Str(value),
                        ..
                    }),
                ..
            }) = argument
            {
                if path.is_ident("prost_path") {
                    found = Some(syn::parse_str::<Path>(&value.value()).ok()?);
                }
            }
        }
    }
    Some(found.unwrap_or_else(|| syn::parse_quote!(::prost)))
}
