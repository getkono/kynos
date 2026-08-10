//! Expansion of `#[derive(Tag)]`.

use proc_macro::TokenStream;

/// Expands the derive into a tag.
pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let _ = item;
    todo!()
}
