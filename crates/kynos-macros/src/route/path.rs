//! Expansion of `path!("/users/{id}")`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{LitStr, parse_macro_input};

/// Expands `path!("/users/{id}")`.
pub(crate) fn expand_path(input: TokenStream) -> TokenStream {
    let literal = parse_macro_input!(input as LitStr);

    match kynos_openapi::PathTemplate::parse(literal.value()) {
        Ok(_) => {}
        Err(error) => {
            return syn::Error::new(literal.span(), error.to_string())
                .to_compile_error()
                .into();
        }
    }

    let raw = literal.value();
    quote! {
        ::kynos::openapi::PathTemplate::parse(#raw)
            .expect("the macro validated this template at compile time")
    }
    .into()
}
