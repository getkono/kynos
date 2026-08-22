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
pub mod negotiate;
pub mod range;
pub mod status;

// RFC 2046 delimiters, factored out of `codec::multipart` so a second subtype
// does not write its own. Gated with the only writer there is today; a second
// one widens the gate rather than moving the module.
#[cfg(feature = "multipart")]
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
/// Bound on every handler return type. Together with
/// [`IntoResponse`] this is the pair that makes the description total: one
/// says what goes on the wire, the other says what the document claims, and a
/// type must supply both.
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
/// [`Responses`] already says what such a type describes, but it says it by
/// building a value from a [`Registry`], and a `const fn` cannot call it. Two
/// interceptors covering one operation and claiming the same status is a
/// conflict worth catching while the program is compiled rather than while the
/// router is built, so the statuses are also available as a `const`.
///
/// # Keeping the two in step
///
/// `STATUSES` and [`Responses::responses`] are two statements of one fact, so
/// they can disagree. Two things stop that mattering:
///
/// * `#[derive(kynos::ApiError)]` emits this implementation from the statuses
///   it already reads, so the ordinary path cannot disagree with itself.
/// * A hand-written implementation is checked while the router is built, and a
///   mismatch is reported as
///   [`SpecError::ShortCircuitMismatch`](kynos_openapi::SpecError::ShortCircuitMismatch).
///
/// Which leaves the const as an optimisation of a fact rather than a second
/// source of it.
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
/// Wildcard patterns and `default` are skipped: they are ranges rather than
/// claims about one status, and a short circuit that answers with a range has
/// nothing exact for the conflict check to compare.
#[must_use]
pub fn described_statuses(responses: &kynos_openapi::Responses) -> Vec<u16> {
    responses
        .responses
        .keys()
        .filter_map(|key| key.parse::<u16>().ok())
        .collect()
}

/// Checks that a [`ShortCircuit`]'s const and its responses agree.
///
/// Returns the violation when they do not. Called while the router is built,
/// which is the only place both halves are available at once — the const is
/// visible to the compiler and the responses need a [`Registry`].
///
/// Always `None` for a type whose implementation the `ApiError` derive emitted,
/// since both halves come from one declaration there.
#[must_use]
pub fn short_circuit_mismatch<S: ShortCircuit>(
    registry: &mut Registry,
) -> Option<kynos_openapi::SpecError> {
    mismatch_between(
        std::any::type_name::<S>(),
        S::STATUSES,
        &S::responses(registry),
    )
}

/// The comparison itself, without the type parameter.
///
/// Split out so it can be tested against hand-built values: the generic form
/// needs a `Responses` produced from a `Registry`, and what is worth asserting
/// is the comparison, not the plumbing.
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
/// 204 is a claim of its own — that there is no content and none is coming —
/// and [`NoContent`](status::NoContent) is how a handler makes it. A handler
/// that returned nothing at all was not asked which it meant, so it gets the
/// status a body type has when no wrapper changes it.
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

/// The uninhabited type, which no extractor that names it can ever produce.
///
/// Present so that an infallible extractor can say so in its `Rejection`
/// rather than inventing an error it never returns.
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
/// This is where a handler's success and failure descriptions come together: a
/// `Result<Json<User>, ApiError>` documents 200 alongside every status
/// `ApiError` can produce, with no restatement anywhere.
///
/// # When both sides claim one status
///
/// The success side wins, and the failure side's entry for that status is
/// dropped. Two reasons: it is the rule
/// [`kynos_openapi::Responses::merge_from`] already applies everywhere else a
/// description is joined, so a status has one meaning throughout the document;
/// and a description keys responses by status alone, so of the two only one can
/// be emitted whatever is chosen here. An error type sharing a status with the
/// success type is asking for a status that means two things, which is a
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
mod tests {
    use std::convert::Infallible;

    use super::{ShortCircuit, described_statuses, mismatch_between, short_circuit_mismatch};
    use crate::schema::registry::Registry;

    fn responses(statuses: &[u16]) -> kynos_openapi::Responses {
        statuses
            .iter()
            .fold(kynos_openapi::Responses::new(), |responses, &status| {
                responses.with(status, kynos_openapi::Response::new("a response"))
            })
    }

    #[test]
    fn only_exact_codes_are_compared() {
        let mut declared = responses(&[503]);
        declared = declared.with_pattern(
            kynos_openapi::StatusPattern::ServerError,
            kynos_openapi::RefOr::Item(kynos_openapi::Response::new("any server error")),
        );
        declared = declared.with_default(kynos_openapi::Response::new("anything else"));

        // `5XX` and `default` are ranges, not claims about one status, so a
        // short circuit has nothing exact to be compared against there.
        assert_eq!(described_statuses(&declared), vec![503]);
    }

    #[test]
    fn agreement_in_any_order_is_agreement() {
        assert!(mismatch_between("Limits", &[503, 429], &responses(&[429, 503])).is_none());
        assert!(mismatch_between("Repeats", &[503, 503], &responses(&[503])).is_none());
    }

    #[test]
    fn a_status_declared_but_not_described_is_reported() {
        let found = mismatch_between("Liar", &[418], &responses(&[503]))
            .expect("418 is declared and never described");

        assert!(matches!(
            found,
            kynos_openapi::SpecError::ShortCircuitMismatch { ref declared, ref described, .. }
                if declared == &[418] && described == &[503]
        ));
        assert!(found.to_string().contains("Liar"));
    }

    #[test]
    fn a_status_described_but_not_declared_is_reported() {
        assert!(mismatch_between("Quiet", &[], &responses(&[500])).is_some());
    }

    #[test]
    fn an_interceptor_that_never_answers_declares_nothing() {
        assert_eq!(<Infallible as ShortCircuit>::STATUSES, &[] as &[u16]);
        assert!(short_circuit_mismatch::<Infallible>(&mut Registry::default()).is_none());
    }
}
