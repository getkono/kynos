//! Expansion of `#[derive(SecurityScheme)]`.

use proc_macro::TokenStream;

/// Expands the derive into a security scheme.
pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let _ = item;
    todo!()
}
