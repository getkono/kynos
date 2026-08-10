//! Inputs drawn from the request head, each describing itself as an OpenAPI
//! Parameter Object.
//!
//! One module per parameter location, so a location that gains a rule gains it
//! in one place. Each pairs a wrapper type — what the handler receives — with
//! the derived trait describing the group it wraps.

pub mod header;
pub mod path;
pub mod query;

#[cfg(feature = "cookie")]
pub mod cookie;
