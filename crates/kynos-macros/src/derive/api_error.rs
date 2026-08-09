//! Expansion of `#[derive(ApiError)]`.

use proc_macro::TokenStream;

/// Expands the derive into the RFC 9457 problem-details mapping for an error type.
pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let _ = item;
    todo!()
}
