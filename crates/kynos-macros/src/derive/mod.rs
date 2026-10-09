//! One module per derive macro, each holding that derive's `expand`; the
//! documented entry points live at the crate root.
//!
//! [`common`] holds what every derive shares, and [`params`] what the four
//! parameter locations share.

pub(crate) mod api_error;
pub(crate) mod common;
pub(crate) mod cookies;
pub(crate) mod headers;
pub(crate) mod multipart;
pub(crate) mod params;
pub(crate) mod path_params;
pub(crate) mod provider;
pub(crate) mod query_params;
pub(crate) mod reply;
pub(crate) mod schema;
pub(crate) mod security_scheme;
pub(crate) mod tag;

#[cfg(test)]
mod tests;
