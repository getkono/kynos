//! Implementation detail of the `kynos-macros` expansions. Not public API.
//!
//! Everything here is `pub` only because expanded code has to name it, and
//! `#[doc(hidden)]` because no human should. Nothing in this module is covered
//! by the crate's compatibility promise; it may change in any release.
//!
//! Its reason to exist is that the alternative — scattering `#[doc(hidden)] pub`
//! items through `router`, `extract` and the rest — puts items no caller can
//! use into modules callers read.

pub mod path;
pub mod uri;

#[cfg(test)]
mod tests;
