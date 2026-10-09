//! Limits, and the responses they make possible.
//!
//! Each limit is its own module, holding the interceptor and the response type
//! it answers with, which both sets and describes its status and headers:
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
