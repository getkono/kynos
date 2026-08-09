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

use crate::{
    error::Rejection,
    extract::{
        FromRequestParts,
        body::{binary::Binary, text::Text},
        describe::Describe,
        media::MediaType,
        params::header::HeaderParams,
    },
    http::{Parts, Response},
    router::OperationCx,
    schema::Registry,
};

#[cfg(feature = "form")]
use crate::extract::body::form::Form;
#[cfg(feature = "multipart")]
use crate::extract::body::multipart::MultipartForm;
#[cfg(feature = "protobuf")]
use crate::extract::body::protobuf::Protobuf;

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

/// A JSON response body, with status 200.
///
/// Requires the default-on `json` feature. Serialization is completed before
/// the response is committed, so a serialization failure becomes a documented
/// RFC 9457 500 response rather than a truncated successful response.
#[cfg(feature = "json")]
pub use crate::extract::body::json::Json;

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

impl<T> Accepted<T> {
    /// Creates a 202 response carrying the accepted work representation.
    pub fn new(body: T) -> Self {
        Self { body }
    }
}

/// A redirect with a status fixed at compile time.
///
/// `CODE` must be one of 301, 302, 303, 307 or 308; anything else fails to
/// compile. That rules out the most common redirect bug, which is using 302
/// where 307 was meant and silently changing the method on replay.
///
/// ```compile_fail
/// fn response<T: kynos::response::IntoResponse>(value: T) { drop(value); }
/// response(kynos::response::Redirect::<304>::to("/cached"));
/// ```
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

/// A compile-time proof that a redirect status is supported.
///
/// Implemented by Kynos for `()` and the five redirect statuses accepted by
/// [`Redirect`]. Downstream crates cannot add implementations because both the
/// trait and `()` are foreign there.
pub trait ValidRedirectCode<const CODE: u16> {}

impl ValidRedirectCode<301> for () {}
impl ValidRedirectCode<302> for () {}
impl ValidRedirectCode<303> for () {}
impl ValidRedirectCode<307> for () {}
impl ValidRedirectCode<308> for () {}

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

impl<T, H> WithHeaders<T, H> {
    /// Attaches a derived header group to a response body.
    pub fn new(body: T, headers: H) -> Self {
        Self { body, headers }
    }
}

/// The client's accepted response representations.
///
/// This extractor contributes no `Accept` parameter because OpenAPI ignores
/// such parameters. It contributes the 406 rejection and the representation
/// tuple contributes the operation's response `content` map.
///
/// ```no_run
/// use kynos::{
///     error::Rejection,
///     extract::{
///         body::{binary::Binary, text::Text},
///         media::Pdf,
///     },
///     response::{Accept, Negotiated},
/// };
///
/// async fn report(
///     accept: Accept<(Text, Binary<Pdf>)>,
/// ) -> Result<Negotiated<(Text, Binary<Pdf>)>, Rejection> {
///     accept.respond((
///         Text("plain report".to_owned()),
///         Binary::new(Vec::<u8>::new()),
///     ))
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accept<T> {
    preferences: Vec<Preference>,
    representations: std::marker::PhantomData<fn() -> T>,
}

impl<T> Accept<T> {
    /// Parses an `Accept` field value for tests and non-server integrations.
    ///
    /// An absent field is represented by `"*/*"`. Invalid quality values are
    /// rejected as malformed headers.
    pub fn parse(value: &str) -> Result<Self, Rejection> {
        let mut preferences = Vec::new();
        for (order, item) in value.split(',').enumerate() {
            let mut segments = item.trim().split(';');
            let range = segments.next().unwrap_or_default().trim();
            let Some((type_, subtype)) = range.split_once('/') else {
                return Err(invalid_accept());
            };
            if type_.is_empty() || subtype.is_empty() || (type_ == "*" && subtype != "*") {
                return Err(invalid_accept());
            }

            let mut quality = 1_000;
            for parameter in segments {
                let Some((name, value)) = parameter.trim().split_once('=') else {
                    return Err(invalid_accept());
                };
                if name.trim().eq_ignore_ascii_case("q") {
                    quality = parse_quality(value.trim()).ok_or_else(invalid_accept)?;
                }
            }
            preferences.push(Preference {
                type_: type_.to_ascii_lowercase(),
                subtype: subtype.to_ascii_lowercase(),
                quality,
                order,
            });
        }
        if preferences.is_empty() {
            return Err(invalid_accept());
        }
        Ok(Self {
            preferences,
            representations: std::marker::PhantomData,
        })
    }

    /// Chooses one offered representation or returns a documented 406.
    pub fn respond(self, representations: T) -> Result<Negotiated<T>, Rejection>
    where
        T: private::Representations,
    {
        let selected = T::media_types()
            .iter()
            .enumerate()
            .filter_map(|(index, media_type)| self.score(media_type).map(|score| (score, index)))
            .max_by(|(left_score, left_index), (right_score, right_index)| {
                left_score
                    .cmp(right_score)
                    .then_with(|| right_index.cmp(left_index))
            })
            .map(|(_, index)| index)
            .ok_or(Rejection::NotAcceptable)?;

        Ok(Negotiated {
            representations,
            selected,
        })
    }

    fn score(&self, media_type: &str) -> Option<(u16, u8, std::cmp::Reverse<usize>)> {
        let (type_, subtype) = media_type.split_once('/')?;
        self.preferences
            .iter()
            .filter_map(|preference| {
                let specificity = if preference.type_ == "*" && preference.subtype == "*" {
                    0
                } else if preference.type_.eq_ignore_ascii_case(type_) && preference.subtype == "*"
                {
                    1
                } else if preference.type_.eq_ignore_ascii_case(type_)
                    && preference.subtype.eq_ignore_ascii_case(subtype)
                {
                    2
                } else {
                    return None;
                };
                Some((
                    specificity,
                    std::cmp::Reverse(preference.order),
                    preference.quality,
                ))
            })
            .max_by_key(|(specificity, order, _)| (*specificity, *order))
            .and_then(|(specificity, order, quality)| {
                (quality != 0).then_some((quality, specificity, order))
            })
    }
}

fn parse_quality(value: &str) -> Option<u16> {
    if value == "0" || value == "0.0" || value == "0.00" || value == "0.000" {
        return Some(0);
    }
    if value == "1" || value == "1.0" || value == "1.00" || value == "1.000" {
        return Some(1_000);
    }
    let digits = value.strip_prefix("0.")?;
    if digits.is_empty() || digits.len() > 3 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits
        .parse::<u16>()
        .ok()
        .map(|quality| match digits.len() {
            1 => quality * 100,
            2 => quality * 10,
            _ => quality,
        })
}

fn invalid_accept() -> Rejection {
    Rejection::Header {
        name: "Accept".to_owned(),
        detail: "expected comma-separated media ranges with q values from 0 to 1".to_owned(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Preference {
    type_: String,
    subtype: String,
    quality: u16,
    order: usize,
}

impl<C: Sync, T: Send> FromRequestParts<C> for Accept<T> {
    type Rejection = Rejection;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (parts, context);
        todo!()
    }
}

impl<T> Describe for Accept<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let responses = Rejection::responses(operation.registry());
        operation.add_responses(responses);
    }
}

/// A response whose representation was chosen from the client's `Accept`
/// header.
///
/// `T` is a tuple of response types, each contributing one entry to the
/// operation's `content` map. Note that `Accept` itself is never declared as a
/// parameter — the specification says such a declaration is ignored, and the
/// `content` map is what actually describes the negotiation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Negotiated<T> {
    representations: T,
    selected: usize,
}

/// A Server-Sent Events response.
///
/// Requires `openapi32`. Under OpenAPI 3.1 an event stream can only be
/// described as an opaque string, which says nothing useful about the events;
/// 3.2's `itemSchema` is what makes each event describable. Kynos would rather
/// not compile than emit a description that lies about your stream.
///
/// ```no_run
/// # #[cfg(feature = "openapi32")]
/// # {
/// use std::time::Duration;
/// use kynos::response::{KeepAlive, Sse};
///
/// fn events<S>(stream: S) -> Sse<S> {
///     Sse::new(stream).keep_alive(
///         KeepAlive::new()
///             .interval(Duration::from_secs(10))
///             .comment("still connected"),
///     )
/// }
/// # }
/// ```
#[cfg(feature = "openapi32")]
#[derive(Debug)]
pub struct Sse<S> {
    /// The stream of events.
    pub events: S,
    keep_alive: Option<KeepAlive>,
}

#[cfg(feature = "openapi32")]
impl<S> Sse<S> {
    /// Creates an event stream without keep-alive messages.
    pub fn new(events: S) -> Self {
        Self {
            events,
            keep_alive: None,
        }
    }

    /// Configures periodic keep-alive comments.
    #[must_use]
    pub fn keep_alive(mut self, keep_alive: KeepAlive) -> Self {
        self.keep_alive = Some(keep_alive);
        self
    }
}

/// Keep-alive configuration for a Server-Sent Events stream.
#[cfg(feature = "openapi32")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeepAlive {
    interval: std::time::Duration,
    comment: String,
}

#[cfg(feature = "openapi32")]
impl KeepAlive {
    /// Creates the default keep-alive configuration.
    pub fn new() -> Self {
        Self {
            interval: std::time::Duration::from_secs(15),
            comment: String::new(),
        }
    }

    /// Sets the interval between keep-alive messages.
    #[must_use]
    pub fn interval(mut self, interval: std::time::Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Sets the comment carried by each keep-alive message.
    #[must_use]
    pub fn comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = comment.into();
        self
    }

    /// Alias for [`comment`](Self::comment), matching axum's builder name.
    #[must_use]
    pub fn text(self, text: impl Into<String>) -> Self {
        self.comment(text)
    }
}

#[cfg(feature = "openapi32")]
impl Default for KeepAlive {
    fn default() -> Self {
        Self::new()
    }
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
    /// A comment sent before the event data, if any.
    pub comment: Option<String>,
}

#[cfg(feature = "openapi32")]
impl<T> Event<T> {
    /// Creates an event carrying typed data.
    pub fn new(data: T) -> Self {
        Self {
            data,
            event: None,
            id: None,
            retry: None,
            comment: None,
        }
    }

    /// Sets the event name.
    #[must_use]
    pub fn event(mut self, event: impl Into<String>) -> Self {
        self.event = Some(event.into());
        self
    }

    /// Sets the event identifier.
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the client reconnection delay in milliseconds.
    #[must_use]
    pub fn retry(mut self, retry: u64) -> Self {
        self.retry = Some(retry);
        self
    }

    /// Adds an SSE comment to this event.
    #[must_use]
    pub fn comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = Some(comment.into());
        self
    }
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

/// A streamed binary response with a declared media type.
///
/// Requires OpenAPI 3.2, whose sequential body vocabulary can describe a
/// representation processed incrementally. Stream failures terminate the body
/// because the successful status has already been committed.
///
/// ```no_run
/// # #[cfg(feature = "openapi32")]
/// # {
/// use kynos::{extract::media::OctetStream, response::BinaryStream};
///
/// fn download<S>(chunks: S) -> BinaryStream<S, OctetStream> {
///     BinaryStream::new(chunks)
/// }
/// # }
/// ```
#[cfg(feature = "openapi32")]
#[derive(Debug)]
pub struct BinaryStream<S, M> {
    /// The stream producing byte chunks.
    pub stream: S,
    media_type: std::marker::PhantomData<M>,
}

#[cfg(feature = "openapi32")]
impl<S, M> BinaryStream<S, M> {
    /// Creates a streamed response from byte chunks.
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            media_type: std::marker::PhantomData,
        }
    }
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

#[cfg(feature = "openapi32")]
impl<S, M, E> IntoResponse for BinaryStream<S, M>
where
    S: futures_core::Stream<Item = Result<bytes::Bytes, E>> + Send + 'static,
    M: MediaType,
    E: Into<Box<dyn std::error::Error + Send + Sync>> + 'static,
{
    fn into_response(self) -> Response {
        todo!()
    }
}

#[cfg(feature = "openapi32")]
impl<S, M> Responses for BinaryStream<S, M>
where
    S: futures_core::Stream,
    M: MediaType,
{
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}

#[cfg(feature = "openapi32")]
impl<S, T, E> IntoResponse for Sse<S>
where
    S: futures_core::Stream<Item = Result<Event<T>, E>> + Send + 'static,
    T: serde::Serialize,
    E: Into<Box<dyn std::error::Error + Send + Sync>> + 'static,
{
    fn into_response(self) -> Response {
        todo!()
    }
}

#[cfg(feature = "openapi32")]
impl<S, T, E> Responses for Sse<S>
where
    S: futures_core::Stream<Item = Result<Event<T>, E>>,
    T: crate::schema::Schema,
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

impl<T: IntoResponse> IntoResponse for Accepted<T> {
    fn into_response(self) -> Response {
        todo!()
    }
}

impl<T: Responses> Responses for Accepted<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}

impl<const CODE: u16> IntoResponse for Redirect<CODE>
where
    (): ValidRedirectCode<CODE>,
{
    fn into_response(self) -> Response {
        todo!()
    }
}

impl<const CODE: u16> Responses for Redirect<CODE>
where
    (): ValidRedirectCode<CODE>,
{
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}

impl<T, H> IntoResponse for WithHeaders<T, H>
where
    T: IntoResponse,
    H: HeaderParams,
{
    fn into_response(self) -> Response {
        todo!()
    }
}

impl<T, H> Responses for WithHeaders<T, H>
where
    T: Responses,
    H: HeaderParams,
{
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}

impl IntoResponse for Text {
    fn into_response(self) -> Response {
        todo!()
    }
}

impl Responses for Text {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}

impl<M: MediaType> IntoResponse for Binary<M> {
    fn into_response(self) -> Response {
        todo!()
    }
}

impl<M: MediaType> Responses for Binary<M> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}

#[cfg(feature = "form")]
impl<T: serde::Serialize> IntoResponse for Form<T> {
    fn into_response(self) -> Response {
        todo!()
    }
}

#[cfg(feature = "form")]
impl<T: crate::schema::Schema> Responses for Form<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}

#[cfg(feature = "multipart")]
impl<T: crate::schema::Schema> IntoResponse for MultipartForm<T> {
    fn into_response(self) -> Response {
        todo!()
    }
}

#[cfg(feature = "multipart")]
impl<T: crate::schema::Schema> Responses for MultipartForm<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}

#[cfg(feature = "protobuf")]
impl<T: prost::Message> IntoResponse for Protobuf<T> {
    fn into_response(self) -> Response {
        todo!()
    }
}

#[cfg(feature = "protobuf")]
impl<T: crate::schema::Schema> Responses for Protobuf<T> {
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

mod private {
    use super::{IntoResponse, MediaType, Registry, Response, Responses};
    use crate::extract::body::{binary::Binary, text::Text};

    #[cfg(feature = "form")]
    use crate::extract::body::form::Form;
    #[cfg(feature = "multipart")]
    use crate::extract::body::multipart::MultipartForm;
    #[cfg(feature = "protobuf")]
    use crate::extract::body::protobuf::Protobuf;

    pub trait Representation: IntoResponse + Responses {
        fn media_type() -> &'static str;
    }

    #[cfg(feature = "json")]
    impl<T> Representation for crate::extract::body::json::Json<T>
    where
        T: serde::Serialize + crate::schema::Schema,
    {
        fn media_type() -> &'static str {
            "application/json"
        }
    }

    impl Representation for Text {
        fn media_type() -> &'static str {
            "text/plain"
        }
    }

    impl<M: MediaType> Representation for Binary<M> {
        fn media_type() -> &'static str {
            M::MEDIA_TYPE
        }
    }

    #[cfg(feature = "form")]
    impl<T> Representation for Form<T>
    where
        T: serde::Serialize + crate::schema::Schema,
    {
        fn media_type() -> &'static str {
            "application/x-www-form-urlencoded"
        }
    }

    #[cfg(feature = "multipart")]
    impl<T: crate::schema::Schema> Representation for MultipartForm<T> {
        fn media_type() -> &'static str {
            "multipart/form-data"
        }
    }

    #[cfg(feature = "protobuf")]
    impl<T> Representation for Protobuf<T>
    where
        T: prost::Message + crate::schema::Schema,
    {
        fn media_type() -> &'static str {
            "application/protobuf"
        }
    }

    pub trait Representations {
        fn media_types() -> Vec<&'static str>;
        fn into_response_at(self, index: usize) -> Response;
        fn responses(registry: &mut Registry) -> kynos_openapi::Responses;
    }

    macro_rules! tuple_representations {
        ($($type:ident : $value:ident = $index:literal),+ $(,)?) => {
            impl<$($type: Representation),+> Representations for ($($type,)+) {
                fn media_types() -> Vec<&'static str> {
                    vec![$($type::media_type()),+]
                }

                fn into_response_at(self, index: usize) -> Response {
                    let ($($value,)+) = self;
                    match index {
                        $($index => $value.into_response(),)+
                        _ => unreachable!("negotiated representation index was validated"),
                    }
                }

                fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
                    let _ = registry;
                    todo!()
                }
            }
        };
    }

    tuple_representations!(A: a = 0, B: b = 1);
    tuple_representations!(A: a = 0, B: b = 1, C: c = 2);
    tuple_representations!(A: a = 0, B: b = 1, C: c = 2, D: d = 3);
    tuple_representations!(A: a = 0, B: b = 1, C: c = 2, D: d = 3, E: e = 4);
    tuple_representations!(A: a = 0, B: b = 1, C: c = 2, D: d = 3, E: e = 4, F: f = 5);
    tuple_representations!(A: a = 0, B: b = 1, C: c = 2, D: d = 3, E: e = 4, F: f = 5, G: g = 6);
    tuple_representations!(A: a = 0, B: b = 1, C: c = 2, D: d = 3, E: e = 4, F: f = 5, G: g = 6, H: h = 7);
}

impl<T: private::Representations> IntoResponse for Negotiated<T> {
    fn into_response(self) -> Response {
        self.representations.into_response_at(self.selected)
    }
}

impl<T: private::Representations> Responses for Negotiated<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        T::responses(registry)
    }
}

#[cfg(test)]
mod tests {
    use super::Accept;
    use crate::extract::{
        body::{binary::Binary, text::Text},
        media::Pdf,
    };

    #[test]
    fn accept_prefers_quality_then_specificity() {
        let accepted = Accept::<(Text, Binary<Pdf>)>::parse("text/*;q=0.5, application/pdf;q=0.9")
            .expect("valid Accept header")
            .respond((Text(String::new()), Binary::default()))
            .expect("a representation matches");

        assert_eq!(accepted.selected, 1);
    }

    #[test]
    fn accept_uses_the_most_specific_range_to_set_quality() {
        let accepted = Accept::<(Text, Binary<Pdf>)>::parse(
            "text/plain;q=0.1, text/*;q=0.9, application/pdf;q=0.5",
        )
        .expect("valid Accept header")
        .respond((Text(String::new()), Binary::default()))
        .expect("a representation matches");

        assert_eq!(accepted.selected, 1);
    }

    #[test]
    fn accept_uses_first_offered_representation_to_break_ties() {
        let accepted = Accept::<(Text, Binary<Pdf>)>::parse("*/*")
            .expect("valid Accept header")
            .respond((Text(String::new()), Binary::default()))
            .expect("a representation matches");

        assert_eq!(accepted.selected, 0);
    }

    #[test]
    fn accept_rejects_zero_quality_and_malformed_values() {
        assert!(
            Accept::<(Text, Binary<Pdf>)>::parse("text/plain;q=0")
                .expect("valid Accept header")
                .respond((Text(String::new()), Binary::default()))
                .is_err()
        );
        assert!(Accept::<(Text, Binary<Pdf>)>::parse("text/plain;q=1.1").is_err());
    }
}
