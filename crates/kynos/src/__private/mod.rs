//! Implementation detail of the `kynos-macros` expansions. Not public API.
//!
//! What lives here is the constraint, endpoint, path, problem, query
//! parameter, reply, string form, temporal and URI support, and the protobuf body type, a route
//! attribute or a derive expands to. An item is `pub` only because
//! expanded code has to name it; a helper no expansion names, such as the
//! percent-coding in [`uri`], stays `pub(crate)`. Nothing in this module is
//! covered by the crate's compatibility promise; it may change in any release.
//!
//! `fuzz` is the one module no expansion names: the `fuzz/` crate's way into
//! parsers that have no public path, present only in a `cargo fuzz` build.
//! [`Refusing`], [`GuardType`] and [`AdmittedReader`] are the items no
//! expansion names: the argument or the answer of a hidden trait method, which
//! a public trait's signature must name.

pub mod constraints;
pub mod endpoint;
// Not a feature, which an application could name.
#[cfg(fuzzing)]
pub mod fuzz;
pub mod path;
pub mod problem;
pub mod query;
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

/// A guard's type, as a hidden `Handler`, `Endpoint` or `Interceptor` item
/// names it, so the router can tell whether an interceptor keyed on what a
/// guard admitted runs inside that guard.
///
/// Only this crate makes one, so no interceptor outside it can claim to read
/// an admission.
#[derive(Clone, Copy, Debug)]
pub struct GuardType {
    id: std::any::TypeId,
    name: &'static str,
}

impl GuardType {
    /// The guard type `G`.
    pub(crate) fn of<G: 'static>() -> Self {
        Self {
            id: std::any::TypeId::of::<G>(),
            name: std::any::type_name::<G>(),
        }
    }

    /// The type's name, as `std::any::type_name` spells it.
    pub(crate) fn name(self) -> &'static str {
        self.name
    }
}

impl PartialEq for GuardType {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for GuardType {}

/// An interceptor an endpoint holds itself that keys on what guard `guard`
/// admitted, as [`Endpoint`](crate::router::endpoint::Endpoint)'s hidden
/// `__admitted_reader` names it.
#[derive(Clone, Copy, Debug)]
pub struct AdmittedReader {
    /// The interceptor's type, as `std::any::type_name` spells it.
    pub(crate) interceptor: &'static str,
    /// The guard it reads.
    pub(crate) guard: GuardType,
}

#[cfg(test)]
mod tests;
