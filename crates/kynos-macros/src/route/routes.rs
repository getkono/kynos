//! Expansion of `routes![a, b, c]`.

use proc_macro::TokenStream;
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

    let _ = paths;
    todo!()
}
