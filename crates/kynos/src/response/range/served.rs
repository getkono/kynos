//! Delivering a representation from a [`ByteSource`].
//!
//! The whole of RFC 9110's conditional-and-ranged algorithm for GET and HEAD,
//! in the order the specification puts it: evaluate the preconditions, then the
//! `Range` field, then send.
//!
//! # Why the order is not a detail
//!
//! Section 14.2 makes the `Range` field conditional on everything before it —
//! it is evaluated "only if the result in absence of the Range header field
//! would be a 200". A 304 therefore wins over a 206, and section 13.1.5's
//! `If-Range` is compared against the validator of the representation being
//! sent rather than against whatever the resource had most recently.
//!
//! # What it does not decide
//!
//! What a failed read means. A source that cannot answer is handed back to the
//! caller, because the application knows whether a missing object is a 404, a
//! 410 or a 500 and Kynos does not.

use std::{sync::Arc, time::SystemTime};

use crate::{
    extract::media::MediaType,
    http::etag::ETag,
    http::{HeaderValue, Method, Parts, Response, StatusCode, header},
    response::{
        IntoResponse,
        disposition::ContentDisposition,
        range::{
            Selection,
            headers::{AcceptRanges, ContentRange},
            source::{ByteSource, Spans},
            spec,
        },
    },
};

/// A representation, ready to be delivered from its source.
///
/// ```no_run
/// use bytes::Bytes;
/// use kynos::{
///     http::etag::ETag,
///     extract::media::OctetStream,
///     response::range::{
///         served::{Conditions, Delivery, Served},
///         source::InMemory,
///     },
/// };
///
/// #[kynos::get("/clips/current")]
/// async fn clip(conditions: Conditions) -> Delivery<OctetStream> {
///     Served::<_, OctetStream>::new(InMemory::new(Bytes::from_static(b"...")))
///         .etag(ETag::strong("r3"))
///         .attachment("clip.mp4")
///         .deliver(&conditions)
///         .await
///         .expect("an in-memory source cannot fail")
/// }
/// ```
#[derive(Debug)]
pub struct Served<S: ByteSource, M: MediaType> {
    source: Arc<S>,
    media_type: std::marker::PhantomData<fn() -> M>,
    etag: Option<ETag>,
    last_modified: Option<SystemTime>,
    cache_control: Option<String>,
    disposition: Option<ContentDisposition>,
}

impl<S: ByteSource, M: MediaType> Served<S, M> {
    /// A representation read from `source`, sent as `M`.
    ///
    /// The media type is a type parameter rather than a string because
    /// [`Delivery`] has to describe itself without a value to look at --
    /// `Responses` is a static method. It is the same reason
    /// [`Binary<M>`](crate::extract::body::binary::Binary) carries one.
    #[must_use]
    pub fn new(source: S) -> Self {
        Self {
            source: Arc::new(source),
            media_type: std::marker::PhantomData,
            etag: None,
            last_modified: None,
            cache_control: None,
            disposition: None,
        }
    }

    /// The validator this representation is known by.
    ///
    /// Without one, `If-Range` cannot be evaluated and section 13.1.5 says a
    /// resume must be answered with the whole representation — so a resumable
    /// download wants a strong tag, and a source that cannot produce one is
    /// telling you it cannot support resumption.
    #[must_use]
    pub fn etag(mut self, etag: ETag) -> Self {
        self.etag = Some(etag);
        self
    }

    /// When the representation last changed.
    ///
    /// The weaker validator, and section 13.1.3 ranks it below `ETag`
    /// accordingly: a one-second resolution cannot distinguish a
    /// representation that changed twice within a second from one that changed
    /// once.
    #[must_use]
    pub fn last_modified(mut self, at: SystemTime) -> Self {
        self.last_modified = Some(at);
        self
    }

    /// How long this representation may be reused.
    #[must_use]
    pub fn cache_control(mut self, value: impl Into<String>) -> Self {
        self.cache_control = Some(value.into());
        self
    }

    /// Sends the representation as a download named `filename`.
    ///
    /// The name is encoded by [`ContentDisposition`], which already owns RFC
    /// 6266 and RFC 8187 — so a filename with a comma, a quote or a non-ASCII
    /// character is safe here rather than being the caller's problem.
    #[must_use]
    pub fn attachment(mut self, filename: impl Into<String>) -> Self {
        self.disposition = Some(ContentDisposition::attachment().filename(filename));
        self
    }

    /// Sends it inline, named.
    #[must_use]
    pub fn inline(mut self, filename: impl Into<String>) -> Self {
        self.disposition = Some(ContentDisposition::inline().filename(filename));
        self
    }

    /// Answers `parts`, reading only what the answer needs.
    ///
    /// # Errors
    ///
    /// Returns the source's own error if the length cannot be read. A failed
    /// *span* read surfaces on the body stream instead, because by then the
    /// status and the fields have been sent and there is nothing left to turn
    /// into a different response. So does a source that stops short of the
    /// length it reported: the `Content-Length` here is sized from
    /// `complete_length`, so a representation truncated after it was measured
    /// fails the body with [`Truncated`](super::source::Truncated) rather than
    /// ending it under a 200 or a 206 that names more octets than arrived.
    pub async fn deliver(self, conditions: &Conditions) -> Result<Delivery<M>, S::Error> {
        let complete_length = self.source.complete_length().await?;

        // Section 13.1: preconditions first, and `If-None-Match` before
        // `If-Modified-Since` -- section 13.1.3 says the date is not evaluated
        // at all when the resource has an entity tag and the request carries
        // one.
        if self.unmodified(conditions) {
            return Ok(Delivery::new(self.head(
                StatusCode::NOT_MODIFIED,
                None,
                complete_length,
            )));
        }

        // Section 14.2: the field is read only where a 200 was owed, which is
        // why this sits after the 304 above.
        let requested = spec::read(
            &conditions.method,
            &conditions.fields,
            self.tag().as_deref(),
        );
        let selection = match crate::response::range::select(&requested, complete_length) {
            Ok(selection) => selection,
            Err(rejection) => return Ok(Delivery::new(rejection.into_response())),
        };

        let (first, last) = match selection {
            Selection::Whole(_) => (0, complete_length.saturating_sub(1)),
            Selection::Part { first, last, .. } => (first, last),
        };

        let mut response = self.head(selection.status(), Some(selection), complete_length);

        // Section 9.3.2: a HEAD is "identical to GET except that the server
        // MUST NOT send content" -- and every field above is sent unchanged,
        // which is what makes a HEAD usable to discover a length before
        // downloading.
        if conditions.method != Method::HEAD && complete_length > 0 {
            *response.body_mut() = crate::http::body::Body::from_body(Spans::new(
                Arc::clone(&self.source),
                first,
                last,
            ));
        }

        Ok(Delivery::new(response))
    }

    /// The response fields, for whichever status is being sent.
    fn head(
        &self,
        status: StatusCode,
        selection: Option<Selection>,
        complete_length: u64,
    ) -> Response {
        let mut response = Response::new(crate::http::body::Body::empty());
        *response.status_mut() = status;
        let fields = response.headers_mut();

        // Section 14.3: a resource that supports ranges says so on every
        // response that carries a representation, which is what lets a client
        // discover resumability without trying it. Not on a 304, which carries
        // none.
        if status != StatusCode::NOT_MODIFIED {
            crate::extract::params::header::write(fields, &AcceptRanges);
            if let Ok(value) = HeaderValue::from_str(M::MEDIA_TYPE) {
                fields.insert(header::CONTENT_TYPE, value);
            }
        }

        if let Some(etag) = self.etag.as_ref().and_then(ETag::encode) {
            fields.insert(header::ETAG, etag);
        }
        if let Some(value) = self
            .last_modified
            .and_then(crate::http::date::format)
            .and_then(|rendered| HeaderValue::from_str(&rendered).ok())
        {
            fields.insert(header::LAST_MODIFIED, value);
        }
        if let Some(value) = self
            .cache_control
            .as_deref()
            .and_then(|value| HeaderValue::from_str(value).ok())
        {
            fields.insert(header::CACHE_CONTROL, value);
        }
        if let Some(disposition) = &self.disposition {
            crate::extract::params::header::write(fields, disposition);
        }

        match selection {
            Some(Selection::Part {
                first,
                last,
                complete_length,
            }) => {
                crate::extract::params::header::write(
                    fields,
                    &ContentRange::Satisfied {
                        first,
                        last,
                        complete_length,
                    },
                );
                if let Ok(value) = HeaderValue::from_str(&(last - first + 1).to_string()) {
                    fields.insert(header::CONTENT_LENGTH, value);
                }
            }
            Some(Selection::Whole(_)) => {
                if let Ok(value) = HeaderValue::from_str(&complete_length.to_string()) {
                    fields.insert(header::CONTENT_LENGTH, value);
                }
            }
            None => {}
        }

        response
    }

    /// The tag as it is spelled on the wire, for `If-Range`.
    fn tag(&self) -> Option<String> {
        self.etag
            .as_ref()
            .and_then(ETag::encode)
            .and_then(|value| value.to_str().ok().map(str::to_owned))
    }

    /// Whether section 13.1's preconditions say the client's copy is current.
    fn unmodified(&self, conditions: &Conditions) -> bool {
        // Section 13.1.3: "A recipient MUST ignore If-Modified-Since if the
        // request contains an If-None-Match header field", so the tag is
        // consulted first and the date only in its absence.
        if let Some(field) = conditions.fields.get(header::IF_NONE_MATCH) {
            return self.tag().is_some_and(|current| matches(field, &current));
        }

        // Section 13.1.3 again: the date condition applies to GET and HEAD
        // alone.
        if conditions.method != Method::GET && conditions.method != Method::HEAD {
            return false;
        }

        let (Some(modified), Some(since)) = (
            self.last_modified,
            conditions
                .fields
                .get(header::IF_MODIFIED_SINCE)
                .and_then(|value| value.to_str().ok())
                .and_then(crate::http::date::parse),
        ) else {
            return false;
        };

        // Compared at one-second resolution, because that is all the field
        // has: a representation whose `Last-Modified` equals the date sent is
        // one the client already has.
        seconds(modified) <= seconds(since)
    }
}

/// Whole seconds since the epoch, or zero for anything before it.
fn seconds(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// Whether `field` names `current`, per RFC 9110 section 13.1.2.
///
/// The *weak* comparison, which is what `If-None-Match` takes: `W/"x"` and
/// `"x"` are the same representation for a cache validation.
fn matches(field: &HeaderValue, current: &str) -> bool {
    let Ok(text) = field.to_str() else {
        return false;
    };

    if text.trim() == crate::http::etag::ANY {
        return true;
    }

    crate::http::etag::split(text)
        .any(|candidate| crate::http::etag::weak_match(candidate, current))
}

/// A delivery, ready to be sent.
///
/// Carries the media type at the type level so it can describe itself:
/// [`Responses`] is a static method, so a `String` field would be invisible to
/// it and the emitted description would have to guess.
///
/// [`Served`] itself is deliberately neither [`IntoResponse`] nor [`Responses`].
/// A delivery is decided *from the request head*, which `into_response` does not
/// have, so a `Served` returned from a handler would answer with the whole
/// representation and ignore every condition the client sent. Leaving the traits
/// unimplemented makes that a compile error rather than a silent wrong answer --
/// the same reason `Rangeable` refuses a `Text`.
///
/// [`Responses`]: crate::response::Responses
#[derive(Debug)]
pub struct Delivery<M: MediaType> {
    response: Response,
    media_type: std::marker::PhantomData<fn() -> M>,
}

impl<M: MediaType> Delivery<M> {
    fn new(response: Response) -> Self {
        Self {
            response,
            media_type: std::marker::PhantomData,
        }
    }

    /// The status this delivery will send.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.response.status()
    }
}

impl<M: MediaType> IntoResponse for Delivery<M> {
    fn into_response(self) -> Response {
        self.response
    }
}

impl<M: MediaType> crate::response::Responses for Delivery<M> {
    fn responses(registry: &mut crate::schema::registry::Registry) -> kynos_openapi::Responses {
        let _ = registry;
        crate::response::range::delivery_responses(M::MEDIA_TYPE)
    }
}

/// The request fields a ranged delivery reads.
///
/// One extractor rather than four, because the four are evaluated together and
/// in an order the specification fixes — a handler that took them separately
/// could apply them in the wrong one. Taking this argument is also what puts
/// `Range`, `If-Range`, `If-None-Match` and `If-Modified-Since` in the emitted
/// description, so an operation that answers a resume says so.
#[derive(Clone, Debug)]
pub struct Conditions {
    /// The method, which decides whether a `Range` is defined at all and
    /// whether content is sent.
    pub(super) method: Method,
    /// The request head, read by `spec::read` and the precondition checks.
    pub(super) fields: crate::http::HeaderMap,
}

impl<C: Sync> crate::extract::FromRequestParts<C> for Conditions {
    type Rejection = std::convert::Infallible;

    /// Infallible. Every unusable value among these four is one the
    /// specification answers by ignoring — section 14.2 for `Range` and
    /// `If-Range`, section 13.1.3 for a malformed `If-Modified-Since` — so
    /// there is no request a client can send that fails to produce a value.
    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        Ok(Self {
            method: parts.method.clone(),
            fields: parts.headers.clone(),
        })
    }
}

impl crate::extract::describe::Describe for Conditions {
    fn describe(operation: &mut crate::router::operation::OperationCx<'_>) {
        operation.add_parameter(crate::response::range::parameter());
        operation.add_parameter(crate::response::range::conditional_parameter());

        for (name, description) in [
            (
                "If-None-Match",
                "The entity tag the client already holds, per RFC 9110 section 13.1.2",
            ),
            (
                "If-Modified-Since",
                "The date the client's copy carries, per RFC 9110 section 13.1.3",
            ),
        ] {
            operation.add_parameter(
                kynos_openapi::Parameter::header(
                    name,
                    kynos_openapi::Schema::of_type(
                        kynos_openapi::model::schema::types::SchemaType::String,
                    ),
                )
                .with_description(description),
            );
        }
    }
}
