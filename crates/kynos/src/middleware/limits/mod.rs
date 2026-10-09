//! Limits, and the responses they make possible.
//!
//! Each limit here owns a type for the response it answers with. That is what
//! keeps the declaration and the behaviour one fact rather than two: the status
//! a limit can produce is the status its response type describes, and a header
//! that rides that status — `Retry-After` on a 503 — is described by the same
//! type that sets it, rather than by a separate entry keyed on the status.
//!
//! Each limit is its own module, holding the interceptor and the response it
//! answers with:
//!
//! - [`body_size`] caps a request body, answering 413.
//! - [`timeout`] caps how long a handler runs, answering 408.
//! - [`concurrency`] caps the requests in flight at once, answering 503.
//! - [`body_timeout`] caps how long a response body takes, and answers nothing:
//!   the head has already left.
//!
//! One limit is not an interceptor: the server holds every request body it
//! reads off a socket to an idle timeout between frames, set with
//! [`Server::request_body_idle_timeout`](crate::server::Server::request_body_idle_timeout).

pub mod body_size;
pub mod body_timeout;
pub mod concurrency;
pub mod timeout;

#[cfg(feature = "server")]
pub(crate) mod request_body;

#[cfg(test)]
mod tests;
