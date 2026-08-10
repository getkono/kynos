//! `#[derive(Reply)]`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, parse_macro_input, spanned::Spanned};

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    // The point of the derive is a *closed set* of responses, one variant per
    // status. A struct has one shape and therefore one status, which the
    // status types in `response::status` already express.
    match &input.data {
        Data::Enum(_) => {}
        Data::Struct(data) => {
            return Err(syn::Error::new(
                data.struct_token.span(),
                "`Reply` declares a closed set of responses, one variant per status, so it needs \
                 an enum. A single response is already expressible: return the body type, or wrap \
                 it in `Created`, `Accepted` or `NoContent`",
            ));
        }
        Data::Union(data) => {
            return Err(syn::Error::new(
                data.union_token.span(),
                "`Reply` needs an enum",
            ));
        }
    }

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics ::kynos::response::IntoResponse for #name #ty_generics #where_clause {
            fn into_response(self) -> ::kynos::http::Response {
                ::core::todo!()
            }
        }

        impl #impl_generics ::kynos::response::Responses for #name #ty_generics #where_clause {
            fn responses(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::kynos::openapi::Responses {
                let _ = registry;
                ::core::todo!()
            }
        }
    })
}
