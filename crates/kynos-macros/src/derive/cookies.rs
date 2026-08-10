//! Expansion of `#[derive(Cookies)]`.

use proc_macro::TokenStream;

/// Expands the derive into a group of request cookies.
pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let _ = item;
    todo!()
}
