//! Turning a handler's return value into a response — and into a Responses
//! Object.
//!
//! # Status codes are types
//!
//! There is no way to choose a status at runtime. `HttpResponse::build(code)`,
//! returning a bare `StatusCode`, `impl IntoResponse` for an ad-hoc tuple —
//! none of these exist, because a status the description does not list is a
//! status the description is wrong about.
//!
//! A handler returning [`Created<Json<User>>`](status::Created) produces 201
//! and says so. A handler that can produce several statuses returns an enum
//! deriving `Reply`, one variant per status.
//!
//! # Headers are part of the type
//!
//! Response headers are declared by wrapping in
//! [`WithHeaders`](headers::WithHeaders), not inserted ad hoc, so
//! `Response.headers` in the description is complete by construction.
//!
//! # How this module is laid out
//!
//! [`status`] holds the responses whose status their type fixes, [`headers`]
//! the header wrapper, [`negotiate`] content negotiation, [`codec`] the
//! responding half of each body codec, and [`stream`] the responses delivered
//! as a sequence.

pub mod codec;
pub mod headers;
pub mod negotiate;
pub mod status;

#[cfg(feature = "openapi32")]
pub mod stream;

use crate::{http::Response, schema::Registry};

/// A value that can be written as an HTTP response.
///
/// Implemented for the response types in this module and for anything deriving
/// `Reply`. There is deliberately no implementation for `String`, `&str`,
/// `StatusCode`, or tuples of them.
///
/// ```compile_fail
/// fn response<T: kynos::response::IntoResponse>(value: T) { drop(value); }
/// response(String::from("the content type would be unknown"));
/// ```
pub trait IntoResponse {
    /// Writes this value as a response.
    fn into_response(self) -> Response;
}

/// A value that can describe every response it may produce.
///
/// Bound on every handler return type. Together with
/// [`IntoResponse`] this is the pair that makes the description total: one
/// says what goes on the wire, the other says what the document claims, and a
/// type must supply both.
pub trait Responses {
    /// The responses this type may produce.
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses;
}

impl IntoResponse for () {
    fn into_response(self) -> Response {
        todo!()
    }
}

impl Responses for () {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}

/// `Result` unions the responses of both sides.
///
/// This is where a handler's success and failure descriptions come together: a
/// `Result<Json<User>, ApiError>` documents 200 alongside every status
/// `ApiError` can produce, with no restatement anywhere.
impl<T, E> Responses for Result<T, E>
where
    T: Responses,
    E: Responses,
{
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}

impl<T, E> IntoResponse for Result<T, E>
where
    T: IntoResponse,
    E: IntoResponse,
{
    fn into_response(self) -> Response {
        match self {
            Ok(value) => value.into_response(),
            Err(error) => error.into_response(),
        }
    }
}
