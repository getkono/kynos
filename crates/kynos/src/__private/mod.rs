//! Implementation detail of the `kynos-macros` expansions. Not public API.
//!
//! What lives here is the constraint, endpoint, path, problem, reply, string
//! form, temporal and URI support, and the protobuf body type, a route
//! attribute or a derive expands to. An item is `pub` only because
//! expanded code has to name it; a helper no expansion names, such as the
//! percent-coding in [`uri`], stays `pub(crate)`. Nothing in this module is
//! covered by the crate's compatibility promise; it may change in any release.
//!
//! `fuzz` is the one module no expansion names: the `fuzz/` crate's way into
//! parsers that have no public path, present only in a `cargo fuzz` build.
//! [`Refusing`] is the one item no expansion names: the argument of a hidden
//! `ShortCircuit` method, which a public trait's signature must name.

pub mod constraints;
pub mod endpoint;
// Not a feature, which an application could name.
#[cfg(fuzzing)]
pub mod fuzz;
pub mod path;
pub mod problem;
pub mod reply;
pub mod string_form;
pub mod temporal;
pub mod uri;

/// The body type `#[derive(ProtobufMessage)]`'s `decode` is handed, named so
/// the expansion needs no `bytes` dependency of the application's.
pub use bytes::Bytes;

/// The problem policy an interceptor's refusal is rendered under: what
/// [`Router::problems`](crate::router::Router::problems) installed, or
/// nothing, handed to
/// [`ShortCircuit::__refused`](crate::response::ShortCircuit::__refused).
#[derive(Clone, Copy, Debug, Default)]
pub struct Refusing(pub(crate) Option<crate::error::members::Policy>);

#[cfg(test)]
mod tests;
