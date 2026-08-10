//! Expansion of `#[derive(QueryParams)]`.

use proc_macro::TokenStream;

/// Expands the derive into a group of query parameters.
pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let _ = item;
    todo!()
}
