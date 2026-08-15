//! One module per derive macro.
//!
//! Each holds that derive's `expand`, so adding a derive is a new file here
//! plus one delegating entry point at the crate root — where `#[proc_macro_*]`
//! items are required to live.
//!
//! Two modules implement no derive of their own: [`common`] holds the shape
//! checks and name handling every derive shares, and [`params`] the reading,
//! writing and describing the four parameter locations differ in only by where
//! a value is found.
//!
//! The documentation for each derive stays on its entry point, next to the
//! trait it implements, rather than being duplicated here.

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
