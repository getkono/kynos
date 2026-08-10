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

    // Each path is written once and used twice: as a type, where it names the
    // marker the route attribute emitted, and as a value, where it names the
    // handler function. That is exactly why the attribute emits a braced
    // struct sharing the function's name — the handler's own type has no
    // spelling a caller could write.
    //
    // The context type and the argument tuple are both inferred: the context
    // from whatever this is mounted into, the arguments from the handler's
    // signature. The mount site is therefore where a handler asking for a
    // dependency the context lacks becomes a compile error.
    let pushes = paths.iter().map(|path| {
        quote! {
            ::kynos::router::endpoint::Endpoints::push(
                &mut __endpoints,
                ::kynos::__private::endpoint::from_meta::<_, #path, _, _>(#path),
            );
        }
    });

    quote! {
        {
            let mut __endpoints = ::kynos::router::endpoint::Endpoints::new();
            #(#pushes)*
            __endpoints
        }
    }
    .into()
}
