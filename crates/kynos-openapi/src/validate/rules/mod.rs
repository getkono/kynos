//! One module per family of specification rule.
//!
//! Each contributes to [`Validator`](crate::validate::Validator) as an inherent
//! `impl` block or free functions [`Validator::validate`] calls.

pub(in crate::validate) mod content;
pub(in crate::validate) mod document;
pub(in crate::validate) mod extensions;
pub(in crate::validate) mod opaque;
pub(in crate::validate) mod operations;
pub(in crate::validate) mod parameters;
pub(in crate::validate) mod paths;
pub(in crate::validate) mod schemas;
