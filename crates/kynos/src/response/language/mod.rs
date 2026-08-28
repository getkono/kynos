//! Choosing a response language from the client's `Accept-Language` field.
//!
//! # Why this is a sibling of `negotiate` rather than part of it
//!
//! [`negotiate`](crate::response::negotiate)'s whole argument is that `Accept`
//! is *never* declared as a parameter, because OpenAPI says such a definition
//! shall be ignored. That is false for this axis: the specification names
//! exactly three such fields — `Accept`, `Content-Type` and `Authorization` —
//! and `Accept-Language` is not among them. Here the parameter is the thing
//! that describes the negotiation, where there it is the `content` map.
//!
//! The two axes are also independent: a response can negotiate on both, and
//! neither type mentions the other.
//!
//! # What OpenAPI can say about a language, which is less than it looks
//!
//! Nothing, directly. Neither 3.1 nor 3.2 has any notion of localization: a
//! description is a single-language artifact, `content` is keyed by media type
//! with no language axis, and there is no way to write "this schema's
//! `description`, in French". What a document *can* carry is the negotiation
//! itself — the `Accept-Language` parameter, the `Content-Language` response
//! header, and the set of tags that header may hold.
//!
//! So the set of tags a service offers is the one thing here that reaches the
//! description, and this module's job is to keep what it sends and what it
//! declared the same set.
//!
//! The strings themselves are the application's. Kynos negotiates; it does not
//! translate, and it ships no catalogue — see
//! [`architecture.md`](../../../../docs/architecture.md)'s third invariant.

pub mod matching;
pub mod tag;

#[cfg(test)]
mod tests;
