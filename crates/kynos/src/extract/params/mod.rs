//! Inputs drawn from the request head, each describing itself as an OpenAPI
//! Parameter Object.
//!
//! One module per parameter location, so a location that gains a rule gains it
//! in one place. Each holds a wrapper type — what the handler receives — and,
//! where the location carries named parameters, the derived trait describing
//! the group it wraps; `querystring` takes the whole query string as one value,
//! so it has no such trait.

pub mod header;
pub mod path;
pub mod query;

#[cfg(feature = "openapi32")]
pub mod querystring;

#[cfg(feature = "cookie")]
pub mod cookie;
