//! Writing a codec type as a response.
//!
//! The types themselves are defined once under
//! [`extract::body`](crate::extract::body) — a codec is one type used in both
//! directions, and defining it twice would be two contracts. What lives here is
//! the responding half: one module per codec, gated the same way its extracting
//! half is.

pub mod binary;
pub mod text;

#[cfg(feature = "form")]
pub mod form;
#[cfg(feature = "json")]
pub mod json;
#[cfg(feature = "multipart")]
pub mod multipart;
#[cfg(feature = "protobuf")]
pub mod protobuf;
