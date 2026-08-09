//! Expansion of `#[derive(Provider)]`.

use proc_macro::TokenStream;

/// Expands the derive into an application context, one `Provides` implementation per field.
pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let _ = item;
    todo!()
}
