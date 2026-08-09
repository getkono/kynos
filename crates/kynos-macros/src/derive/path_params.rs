//! Expansion of `#[derive(PathParams)]`.

use proc_macro::TokenStream;

/// Expands the derive into a group of path parameters.
pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let _ = item;
    todo!()
}
