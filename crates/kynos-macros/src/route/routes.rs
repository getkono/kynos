//! Expansion of `routes![a, b, c]`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Token, parse::Parser, punctuated::Punctuated};

/// Expands `routes![a, b, c]`.
pub(crate) fn expand_routes(input: TokenStream) -> TokenStream {
    let parser = Punctuated::<syn::Path, Token![,]>::parse_terminated;
    let paths = match parser.parse(input) {
        Ok(paths) => paths,
        Err(error) => return error.to_compile_error().into(),
    };

    if paths.is_empty() {
        return syn::Error::new(
            proc_macro2::Span::call_site(),
            "`routes!` needs at least one operation",
        )
        .to_compile_error()
        .into();
    }

    // Each path names both the endpoint type and the handler function. The
    // context and arguments are inferred, so a missing dependency fails at the
    // mount site. A tuple rather than `Endpoints`, so each member keeps the type
    // its interceptors are checked against there.
    let members = paths.iter().map(|path| {
        quote! {
            ::kynos::__private::endpoint::from_meta::<_, #path, _, _>(#path),
        }
    });

    quote! {
        ( #(#members)* )
    }
    .into()
}
