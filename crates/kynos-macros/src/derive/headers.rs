//! Expansion of `#[derive(Headers)]`.

use proc_macro::TokenStream;

/// Expands the derive into a group of request or response headers.
pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let _ = item;
    todo!()
}
