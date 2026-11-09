//! Implementation detail of the `kynos-macros` expansions. Not public API.
//!
//! What lives here is the constraint, endpoint, path, problem, reply and URI
//! support, and the protobuf body type, a route attribute or a derive expands
//! to. An item is `pub` only because
//! expanded code has to name it; a helper no expansion names, such as the
//! percent-coding in [`uri`], stays `pub(crate)`. Nothing in this module is
//! covered by the crate's compatibility promise; it may change in any release.
//!
//! `fuzz` is the one module no expansion names: the `fuzz/` crate's way into
//! parsers that have no public path, present only in a `cargo fuzz` build.

pub mod constraints;
pub mod endpoint;
// Not a feature, which an application could name.
#[cfg(fuzzing)]
pub mod fuzz;
pub mod path;
pub mod problem;
pub mod reply;
pub mod uri;

/// The body type `#[derive(ProtobufMessage)]`'s `decode` is handed, named so
/// the expansion needs no `bytes` dependency of the application's.
pub use bytes::Bytes;

#[cfg(test)]
mod tests;
