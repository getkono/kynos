//! The OpenAPI object model.
//!
//! Data and invariant-preserving constructors only; emission lives in
//! [`crate::emit`] and validation in [`crate::validate`].

pub mod body;
pub mod callback;
pub mod components;
pub mod document;
pub mod example;
pub mod extensions;
pub mod external_docs;
pub mod info;
pub mod link;
// Private: one deserializer for the model's own fields.
mod nullable;
// Private: one deserializer for the model's own fields.
mod number;
pub mod parameter;
pub mod paths;
pub mod reference;
pub mod response;
pub mod schema;
pub mod security;
pub mod server;
pub mod tag;

#[cfg(test)]
mod tests;
