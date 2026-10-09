//! Implementation detail of the `kynos-macros` expansions. Not public API.
//!
//! What lives here is the endpoint, path, problem, reply and URI support a
//! route attribute or a derive expands to. An item is `pub` only because
//! expanded code has to name it, and `#[doc(hidden)]` because no human should;
//! a helper the crate shares with that support but no expansion names, such as
//! the percent-coding in [`uri`], stays `pub(crate)`. Nothing in this module is
//! covered by the crate's compatibility promise; it may change in any release.
//!
//! `fuzz` is the one module no expansion names: the `fuzz/` crate's way into
//! parsers that have no public path, present only in a `cargo fuzz` build.
//!
//! Its reason to exist is that the alternative — scattering `#[doc(hidden)] pub`
//! items through `router`, `extract` and the rest — puts items no caller can
//! use into modules callers read.

pub mod endpoint;
// `cfg(fuzzing)` rather than a feature: a feature is a flag an application can
// name, and this module is the `fuzz/` crate's alone.
#[cfg(fuzzing)]
pub mod fuzz;
pub mod path;
pub mod problem;
pub mod reply;
pub mod uri;

#[cfg(test)]
mod tests;
