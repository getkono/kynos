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
//! the header wrapper, [`disposition`] the header group that says whether a
//! representation is saved or shown, [`negotiate`] content negotiation,
//! [`range`] the one part of a representation a request asked for, [`codec`]
//! the responding half of each body codec, and [`stream`] the responses
//! delivered as a sequence.

use core::convert::Infallible;

pub mod codec;
#[cfg(feature = "cookie")]
pub mod cookie;
pub mod disposition;
pub mod headers;
pub mod language;
pub mod negotiate;
pub mod range;
pub mod status;

// RFC 2046 delimiters, shared by both multipart subtypes Kynos writes.
#[cfg(any(feature = "multipart", feature = "openapi32"))]
mod framing;

#[cfg(feature = "openapi32")]
pub mod stream;

use crate::{
    http::{Response, body::Body},
    schema::registry::Registry,
};

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
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be turned into a response",
    label = "not a response",
    note = "return a body type, or wrap one in `Created`, `Accepted`, `NoContent` or `Redirect`",
    note = "a bare `StatusCode` is deliberately not one: a status the description does not list \
            is a status it is wrong about. Use `#[derive(Reply)]` when an operation has several"
)]
pub trait IntoResponse {
    /// Writes this value as a response.
    fn into_response(self) -> Response;
}

/// A value that can describe every response it may produce.
///
/// Bound on every handler return type, beside [`IntoResponse`]: one says what
/// the document claims, the other what goes on the wire.
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not declare which responses it can produce",
    label = "undeclared responses",
    note = "a handler's return type has to say what a consumer might receive; derive it with \
            `#[derive(kynos::Reply)]`, or `#[derive(kynos::ApiError)]` for an error type"
)]
pub trait Responses {
    /// The responses this type may produce.
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses;
}

/// A response an interceptor can produce without reaching the handler.
///
/// `STATUSES` restates [`Responses`] as a `const`, so two interceptors claiming
/// the same status on one operation are caught at compile time.
///
/// `#[derive(kynos::ApiError)]` emits both from one declaration. A hand-written
/// implementation is checked while the router is built, and a mismatch is
/// [`SpecError::ShortCircuitMismatch`](kynos_openapi::SpecError::ShortCircuitMismatch).
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be an interceptor's short circuit",
    label = "not a short circuit",
    note = "an interceptor answers with a type that says which statuses it can produce; derive it \
            with `#[derive(kynos::ApiError)]`",
    note = "use `std::convert::Infallible` for an interceptor that always reaches the handler"
)]
pub trait ShortCircuit: IntoResponse + Responses {
    /// The statuses this type can answer with.
    const STATUSES: &'static [u16];
}

/// The statuses a [`kynos_openapi::Responses`] value declares as exact codes.
///
/// Wildcard patterns and `default` are skipped: they name no one status.
#[must_use]
pub(crate) fn described_statuses(responses: &kynos_openapi::Responses) -> Vec<u16> {
    responses
        .responses
        .keys()
        .filter_map(|key| key.parse::<u16>().ok())
        .collect()
}

/// Checks that a [`ShortCircuit`]'s const and its responses agree.
///
/// Returns the violation when they do not. Called while the router is built,
/// where a [`Registry`] exists.
#[must_use]
pub(crate) fn short_circuit_mismatch<S: ShortCircuit>(
    registry: &mut Registry,
) -> Option<kynos_openapi::SpecError> {
    mismatch_between(
        std::any::type_name::<S>(),
        S::STATUSES,
        &S::responses(registry),
    )
}

/// The comparison itself, without the type parameter, for testing.
fn mismatch_between(
    name: &str,
    statuses: &[u16],
    responses: &kynos_openapi::Responses,
) -> Option<kynos_openapi::SpecError> {
    let normalize = |mut codes: Vec<u16>| {
        codes.sort_unstable();
        codes.dedup();
        codes
    };

    let declared = normalize(statuses.to_vec());
    let described = normalize(described_statuses(responses));

    if declared == described {
        return None;
    }

    Some(kynos_openapi::SpecError::ShortCircuitMismatch {
        name: name.to_owned(),
        declared,
        described,
    })
}

/// The empty body, which is 200 like every other bare body type.
///
/// Return [`NoContent`](status::NoContent) for a 204.
impl IntoResponse for () {
    fn into_response(self) -> Response {
        Response::new(Body::empty())
    }
}

/// Describes that 200, with no content: there is no representation to name.
impl Responses for () {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        kynos_openapi::Responses::new().with(
            200,
            kynos_openapi::Response::new("the request succeeded, and the response has no body"),
        )
    }
}

/// The uninhabited type, so an infallible extractor can name it as its
/// `Rejection`.
impl IntoResponse for Infallible {
    fn into_response(self) -> Response {
        match self {}
    }
}

/// Contributes no responses, because there are none to contribute.
impl Responses for Infallible {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        kynos_openapi::Responses::new()
    }
}

/// The short circuit of an interceptor that never short-circuits.
///
/// An empty `STATUSES` conflicts with nothing, so a pass-through interceptor
/// composes with every other one and adds nothing to any description.
impl ShortCircuit for Infallible {
    const STATUSES: &'static [u16] = &[];
}

/// `Result` unions the responses of both sides.
///
/// A `Result<Json<User>, ApiError>` documents 200 alongside every status
/// `ApiError` can produce.
///
/// # When both sides claim one status
///
/// The success side wins, through
/// [`kynos_openapi::Responses::merge_from`], and the failure side's entry is
/// dropped. A status that means two things calls for a
/// [`Reply`](crate::Reply) enum rather than a `Result`.
impl<T, E> Responses for Result<T, E>
where
    T: Responses,
    E: Responses,
{
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let mut responses = T::responses(registry);
        responses.merge_from(&E::responses(registry));
        responses
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

#[cfg(test)]
mod tests;
