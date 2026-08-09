//! Expansion of `#[derive(Reply)]`.

use proc_macro::TokenStream;

/// Expands the derive into a closed set of responses, one variant per status.
pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let _ = item;
    todo!()
}
