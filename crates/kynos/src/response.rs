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
//! A handler returning [`Created<Json<User>>`](Created) produces 201 and says
//! so. A handler that can produce several statuses returns an enum deriving
//! `Reply`, one variant per status.
//!
//! # Headers are part of the type
//!
//! Response headers are declared by wrapping in [`WithHeaders`], not inserted
//! ad hoc, so `Response.headers` in the description is complete by
//! construction.

use crate::{http::Response, schema::Registry};

/// A value that can be written as an HTTP response.
///
/// Implemented for the response types in this module and for anything deriving
/// `Reply`. There is deliberately no implementation for `String`, `&str`,
/// `StatusCode`, or tuples of them.
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

/// A JSON response body, with status 200.
///
/// Requires the default-on `json` feature. Serialization is completed before
/// the response is committed, so a serialization failure becomes a documented
/// RFC 9457 500 response rather than a truncated successful response.
#[cfg(feature = "json")]
pub use crate::extract::Json;

/// A 204 No Content response.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NoContent;

/// A 201 Created response carrying the created representation.
///
/// The `Location` header is required rather than optional: a 201 without one
/// tells a client something was created but not where, which is rarely what
/// anybody wants and is trivial to forget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Created<T> {
    /// The created representation.
    pub body: T,
    /// Where the new resource lives.
    pub location: String,
}

impl<T> Created<T> {
    /// Creates a 201 response for a resource at `location`.
    pub fn at(location: impl Into<String>, body: T) -> Self {
        Self {
            body,
            location: location.into(),
        }
    }
}

/// A 202 Accepted response for work that has not finished.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Accepted<T> {
    /// A representation of the accepted work, typically a job handle.
    pub body: T,
}

/// A redirect with a status fixed at compile time.
///
/// `CODE` must be one of 301, 302, 303, 307 or 308; anything else fails to
/// compile. That rules out the most common redirect bug, which is using 302
/// where 307 was meant and silently changing the method on replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Redirect<const CODE: u16> {
    /// The target of the redirect.
    pub location: String,
}

impl<const CODE: u16> Redirect<CODE> {
    /// Redirects to `location`.
    pub fn to(location: impl Into<String>) -> Self {
        Self {
            location: location.into(),
        }
    }
}

/// A response carrying declared headers alongside its body.
///
/// `H` derives `Headers`, so each header appears in `Response.headers` with its
/// own schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WithHeaders<T, H> {
    /// The response body.
    pub body: T,
    /// The declared headers.
    pub headers: H,
}

/// A response whose representation is chosen by the client's `Accept` header.
///
/// `T` is a tuple of response types, each contributing one entry to the
/// operation's `content` map. Note that `Accept` itself is never declared as a
/// parameter — the specification says such a declaration is ignored, and the
/// `content` map is what actually describes the negotiation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Negotiated<T>(pub T);

/// A Server-Sent Events response.
///
/// Requires `openapi32`. Under OpenAPI 3.1 an event stream can only be
/// described as an opaque string, which says nothing useful about the events;
/// 3.2's `itemSchema` is what makes each event describable. Kynos would rather
/// not compile than emit a description that lies about your stream.
#[cfg(feature = "openapi32")]
#[derive(Debug)]
pub struct Sse<S> {
    /// The stream of events.
    pub events: S,
}

/// One Server-Sent Event.
#[cfg(feature = "openapi32")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Event<T> {
    /// The event payload, described by `itemSchema`.
    pub data: T,
    /// The event name, matched by a client's listener.
    pub event: Option<String>,
    /// The event id, which a client returns as `Last-Event-ID` on reconnect.
    pub id: Option<String>,
    /// How long a client should wait before reconnecting, in milliseconds.
    pub retry: Option<u64>,
}

/// A newline-delimited JSON response (`application/x-ndjson`).
///
/// Requires both `json` and `openapi32`; the latter supplies the `itemSchema`
/// needed to describe each streamed value.
///
/// ```no_run
/// # #[cfg(all(feature = "json", feature = "openapi32"))]
/// # {
/// use kynos::response::JsonLines;
///
/// fn lines<S>(items: S) -> JsonLines<S> {
///     JsonLines { items }
/// }
/// # }
/// ```
#[cfg(all(feature = "json", feature = "openapi32"))]
#[derive(Debug)]
pub struct JsonLines<S> {
    /// The stream of items.
    pub items: S,
}

/// An RFC 7464 JSON text sequence response (`application/json-seq`).
///
/// Requires both `json` and `openapi32`; the latter supplies the `itemSchema`
/// needed to describe each streamed value.
///
/// ```no_run
/// # #[cfg(all(feature = "json", feature = "openapi32"))]
/// # {
/// use kynos::response::JsonSeq;
///
/// fn sequence<S>(items: S) -> JsonSeq<S> {
///     JsonSeq { items }
/// }
/// # }
/// ```
#[cfg(all(feature = "json", feature = "openapi32"))]
#[derive(Debug)]
pub struct JsonSeq<S> {
    /// The stream of items.
    pub items: S,
}

impl IntoResponse for NoContent {
    fn into_response(self) -> Response {
        todo!()
    }
}

impl Responses for NoContent {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}

#[cfg(feature = "json")]
impl<T: serde::Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        todo!()
    }
}

#[cfg(feature = "json")]
impl<T: crate::schema::Schema> Responses for Json<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}

/// Streams each item as one JSON value followed by a newline.
///
/// The response is committed before every item is available. If serializing a
/// later item fails, the stream terminates; it cannot replace the already-sent
/// status with a problem response.
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<S> IntoResponse for JsonLines<S>
where
    S: futures_core::Stream + Send + 'static,
    S::Item: serde::Serialize,
{
    fn into_response(self) -> Response {
        todo!()
    }
}

#[cfg(all(feature = "json", feature = "openapi32"))]
impl<S> Responses for JsonLines<S>
where
    S: futures_core::Stream,
    S::Item: crate::schema::Schema,
{
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}

/// Streams each item as an RFC 7464 JSON text sequence record.
///
/// The response is committed before every item is available. If serializing a
/// later item fails, the stream terminates; it cannot replace the already-sent
/// status with a problem response.
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<S> IntoResponse for JsonSeq<S>
where
    S: futures_core::Stream + Send + 'static,
    S::Item: serde::Serialize,
{
    fn into_response(self) -> Response {
        todo!()
    }
}

#[cfg(all(feature = "json", feature = "openapi32"))]
impl<S> Responses for JsonSeq<S>
where
    S: futures_core::Stream,
    S::Item: crate::schema::Schema,
{
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}

impl<T: IntoResponse> IntoResponse for Created<T> {
    fn into_response(self) -> Response {
        todo!()
    }
}

impl<T: Responses> Responses for Created<T> {
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
