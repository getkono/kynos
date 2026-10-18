//! Expansion of `path!("/users/{id}")`.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{LitStr, parse_macro_input};

/// Expands `path!("/users/{id}")`.
pub(crate) fn expand_path(input: TokenStream) -> TokenStream {
    let literal = parse_macro_input!(input as LitStr);
    expand_template(&literal)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// The expression `path!` expands `literal` to, once it parses as a template.
pub(super) fn expand_template(literal: &LitStr) -> syn::Result<TokenStream2> {
    let raw = literal.value();
    kynos_openapi::PathTemplate::parse(raw.clone())
        .map_err(|error| syn::Error::new(literal.span(), error.to_string()))?;

    Ok(quote! {
        ::kynos::openapi::PathTemplate::parse(#raw)
            .expect("the macro validated this template at compile time")
    })
}
