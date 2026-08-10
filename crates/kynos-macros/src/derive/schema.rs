//! Expansion of `#[derive(Schema)]`.

use proc_macro::TokenStream;

/// Expands the derive into the `Schema` implementation for a type.
pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let _ = item;
    todo!()
}
