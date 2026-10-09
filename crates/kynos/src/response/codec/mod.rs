//! Writing a codec type as a response.
//!
//! The codec types are defined once under
//! [`extract::body`](crate::extract::body); this is their responding half.

// Private where they declare no item; `multipart` declares `IntoMultipart`.
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
