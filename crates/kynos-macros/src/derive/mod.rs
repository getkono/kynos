//! One module per derive macro, each holding that derive's `expand_inner`;
//! the documented entry points live at the crate root, and reach it through
//! [`run`].
//!
//! [`common`] holds what every derive shares, [`params`] what the four
//! parameter locations share, and [`attribute`] how a helper attribute's
//! closed key set is read.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;

pub(crate) mod api_error;
pub(crate) mod attribute;
pub(crate) mod common;
pub(crate) mod cookies;
pub(crate) mod headers;
pub(crate) mod multipart;
pub(crate) mod param_value;
pub(crate) mod params;
pub(crate) mod path_params;
pub(crate) mod provider;
pub(crate) mod query_params;
pub(crate) mod reply;
pub(crate) mod schema;
pub(crate) mod security_scheme;
pub(crate) mod tag;

/// Parses a macro's input as `T` and expands it, rendering either failure as
/// the `compile_error!` the user sees.
pub(crate) fn run<T: syn::parse::Parse>(
    item: TokenStream,
    expand: impl FnOnce(&T) -> syn::Result<TokenStream2>,
) -> TokenStream {
    match syn::parse::<T>(item).and_then(|input| expand(&input)) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

#[cfg(test)]
mod tests;
