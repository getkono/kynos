//! Inputs drawn from the request head, each describing itself as an OpenAPI
//! Parameter Object.
//!
//! One module per parameter location, each holding the wrapper a handler
//! receives and, for named parameters, the derived trait describing the group.

pub mod header;
pub mod path;
pub mod query;

#[cfg(feature = "openapi32")]
pub mod querystring;

#[cfg(feature = "cookie")]
pub mod cookie;
