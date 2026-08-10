//! One module per family of specification rule.
//!
//! Each submodule owns the checks for the objects it names, and contributes to
//! [`Validator`](crate::validate::Validator) either as an inherent `impl` block
//! or as free functions the orchestrator calls. Adding a rule family means
//! adding a file here and one call in [`Validator::validate`].

pub(in crate::validate) mod content;
pub(in crate::validate) mod document;
pub(in crate::validate) mod extensions;
pub(in crate::validate) mod operations;
pub(in crate::validate) mod parameters;
pub(in crate::validate) mod paths;
