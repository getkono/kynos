//! Delivering a representation from a [`ByteSource`].
//!
//! The whole of RFC 9110's conditional-and-ranged algorithm for GET and HEAD,
//! in the order section 13.2.2 puts it: `If-Match`, else `If-Unmodified-Since`;
//! then `If-None-Match`, else `If-Modified-Since`; then `If-Range` and the
//! `Range` field it guards; then send.
//!
//! `Range` is evaluated only where a 200 was otherwise owed (section 14.2), so
//! a 304 wins over a 206.
//!
//! A source that cannot answer is handed back to the caller, which knows
//! whether a missing object is a 404, a 410 or a 500.

use std::{sync::Arc, time::SystemTime};

use crate::{
    http::etag::ETag,
    http::media::MediaType,
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
///     http::media::OctetStream,
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
    /// `M` is a type parameter so that [`Delivery`] can describe itself
    /// statically.
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
    /// A resumable download needs a strong tag: without one, `If-Range` cannot
    /// hold and a resume gets the whole representation (section 13.1.5).
    #[must_use]
    pub fn etag(mut self, etag: ETag) -> Self {
        self.etag = Some(etag);
        self
    }

    /// When the representation last changed.
    ///
    /// The weaker validator, at one-second resolution; section 13.1.3 ranks it
    /// below `ETag`.
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
    /// The name is encoded by [`ContentDisposition`] (RFC 6266, RFC 8187), so
    /// any filename is safe.
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
    /// span read, or a source that stops short of the length it reported
    /// ([`Truncated`](super::source::Truncated)), fails the body stream instead,
    /// since the head has already been sent.
    pub async fn deliver(self, conditions: &Conditions) -> Result<Delivery<M>, S::Error> {
        let complete_length = self.source.complete_length().await?;

        // Section 13.2.2 steps 1 and 2: the lost-update preconditions first.
        if self.precondition_failed(conditions) {
            let mut response = Response::new(crate::http::body::Body::empty());
            *response.status_mut() = StatusCode::PRECONDITION_FAILED;
            return Ok(Delivery::new(response));
        }

        // Steps 3 and 4: the cache validations.
        if self.unmodified(conditions) {
            return Ok(Delivery::new(self.head(
                StatusCode::NOT_MODIFIED,
                None,
                complete_length,
            )));
        }

        // Section 14.2: the field is read only where a 200 was owed.
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

        // Section 9.3.2: a HEAD sends every field above but no content.
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

        // Section 14.3: advertised on every response carrying a representation,
        // which a 304 does not.
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

    /// Whether section 13.2.2's first two steps refuse the request.
    ///
    /// `If-Unmodified-Since` is ignored when `If-Match` is present (section
    /// 13.1.4).
    fn precondition_failed(&self, conditions: &Conditions) -> bool {
        // Section 13.1.1, the same evaluation an asset makes.
        if let Some(holds) = crate::http::etag::if_match(&conditions.fields, || self.tag()) {
            return !holds;
        }

        // Section 13.1.4: ignored unless exactly one HTTP-date and a
        // modification date to compare.
        let mut lines = conditions
            .fields
            .get_all(header::IF_UNMODIFIED_SINCE)
            .iter();
        let (Some(line), None) = (lines.next(), lines.next()) else {
            return false;
        };
        let (Some(modified), Some(since)) = (
            self.last_modified,
            line.to_str().ok().and_then(crate::http::date::parse),
        ) else {
            return false;
        };

        seconds(modified) > seconds(since)
    }

    /// Whether section 13.1's preconditions say the client's copy is current.
    fn unmodified(&self, conditions: &Conditions) -> bool {
        // Section 13.1.3: `If-None-Match` overrides `If-Modified-Since`.
        if let Some(field) = conditions.fields.get(header::IF_NONE_MATCH) {
            return self
                .tag()
                .is_some_and(|current| crate::http::etag::matches(field, &current));
        }

        // Section 13.1.3: the date applies to GET and HEAD alone.
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

        // At the field's one-second resolution.
        seconds(modified) <= seconds(since)
    }
}

/// Whole seconds since the epoch, or zero for anything before it.
fn seconds(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// A delivery, ready to be sent.
///
/// [`Served`] is deliberately neither [`IntoResponse`] nor [`Responses`]: a
/// delivery is decided from the request head, so returning a `Served` without
/// calling [`deliver`](Served::deliver) is a compile error.
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
        crate::response::range::delivery_responses(registry, M::MEDIA_TYPE)
    }
}

/// The request fields a ranged delivery reads.
///
/// One extractor rather than six, because the specification fixes the order
/// they are evaluated in. Taking it declares `Range`, `If-Range`, `If-Match`,
/// `If-Unmodified-Since`, `If-None-Match` and `If-Modified-Since`.
#[derive(Clone, Debug)]
pub struct Conditions {
    /// The method.
    pub(super) method: Method,
    /// The request head, read by `spec::read` and the precondition checks.
    pub(super) fields: crate::http::HeaderMap,
}

impl<C: Sync> crate::extract::FromRequestParts<C> for Conditions {
    type Rejection = std::convert::Infallible;

    /// Infallible: every unusable value among the six is ignored or fails its
    /// condition (RFC 9110 sections 13.1, 14.2).
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
                "If-Match",
                "The entity tag the client's copy was taken from, per RFC 9110 section 13.1.1",
            ),
            (
                "If-Unmodified-Since",
                "The date the client's copy carries, refused if the representation changed since, \
                 per RFC 9110 section 13.1.4",
            ),
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
