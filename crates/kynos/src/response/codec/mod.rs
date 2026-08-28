//! Writing a codec type as a response.
//!
//! The types themselves are defined once under
//! [`extract::body`](crate::extract::body) — a codec is one type used in both
//! directions, and defining it twice would be two contracts. What lives here is
//! the responding half: one module per codec, gated the same way its extracting
//! half is.

// Private, for the reason `schema::impls` gives: each of these declares no
// item, only the responding half of a codec whose type lives under
// `extract::body`, so there is nothing here for a canonical path to point at.
// `multipart` is the exception -- it declares `IntoMultipart` and `IntoPart`.
mod binary;
mod text;

#[cfg(feature = "form")]
mod form;
#[cfg(feature = "json")]
mod json;
#[cfg(feature = "multipart")]
pub mod multipart;
#[cfg(feature = "protobuf")]
mod protobuf;

#[cfg(test)]
mod tests;
