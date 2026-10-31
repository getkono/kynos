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
    http::media::{MediaRanges, MediaType, WellFormed, WellFormedRanges},
    http::{HeaderValue, Method, Parts, Response, StatusCode, body::bodiless, header},
    response::{
        IntoResponse,
        disposition::ContentDisposition,
        range::{
            Selection,
            delivery::{self, Precondition},
            source::{ByteSource, Spans},
            spec, write_selection,
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
///
/// A representation whose type is known only at request time names the set it
/// can be with [`MediaRanges`], and gives its type to
/// [`content_type`](Self::content_type):
///
/// ```no_run
/// use bytes::Bytes;
/// use kynos::{
///     http::media::MediaRanges,
///     response::range::{
///         served::{Conditions, Delivery, Served},
///         source::InMemory,
///     },
/// };
///
/// struct Video;
///
/// impl MediaRanges for Video {
///     const RANGES: &'static [&'static str] = &["video/*"];
/// }
///
/// #[kynos::get("/clips/current")]
/// async fn clip(conditions: Conditions) -> Delivery<Video> {
///     Served::<_, Video>::new(InMemory::new(Bytes::from_static(b"...")))
///         .content_type("video/mp4")
///         .deliver(&conditions)
///         .await
///         .expect("an in-memory source cannot fail")
/// }
/// ```
///
/// A set names no type to send, so one that was never given a type cannot be
/// delivered:
///
/// ```compile_fail
/// use bytes::Bytes;
/// use kynos::{
///     http::media::MediaRanges,
///     response::range::{
///         served::{Conditions, Delivery, Served},
///         source::InMemory,
///     },
/// };
///
/// struct Video;
///
/// impl MediaRanges for Video {
///     const RANGES: &'static [&'static str] = &["video/*"];
/// }
///
/// async fn clip(conditions: Conditions) -> Delivery<Video> {
///     Served::<_, Video>::new(InMemory::new(Bytes::new()))
///         .deliver(&conditions)
///         .await
///         .expect("an in-memory source cannot fail")
/// }
/// ```
pub struct Served<S: ByteSource, M: MediaRanges, T = Declared> {
    source: Arc<S>,
    media_type: std::marker::PhantomData<fn() -> M>,
    content_type: T,
    etag: Option<ETag>,
    last_modified: Option<SystemTime>,
    cache_control: Option<String>,
    disposition: Option<ContentDisposition>,
}

/// A [`Served`] sent as its [`MediaType`] marker states.
#[derive(Clone, Copy, Debug)]
pub struct Declared;

/// A [`Served`] given its media type by [`content_type`](Served::content_type).
#[derive(Clone, Debug)]
pub struct Detected(Option<HeaderValue>);

// By hand, so it is not bounded on the media type: it is a marker, and a derive
// would ask it to be `Debug` too.
impl<S: ByteSource + std::fmt::Debug, M: MediaRanges, T: std::fmt::Debug> std::fmt::Debug
    for Served<S, M, T>
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Served")
            .field("source", &self.source)
            .field("content_type", &self.content_type)
            .field("etag", &self.etag)
            .field("last_modified", &self.last_modified)
            .field("cache_control", &self.cache_control)
            .field("disposition", &self.disposition)
            .finish_non_exhaustive()
    }
}

impl<S: ByteSource, M: MediaRanges> Served<S, M> {
    /// A representation read from `source`, sent as `M`.
    ///
    /// `M` is a type parameter so that [`Delivery`] can describe itself
    /// statically. Where it is a [`MediaRanges`] set rather than a [`MediaType`]
    /// marker, [`content_type`](Self::content_type) says which type this one is.
    #[must_use]
    pub fn new(source: S) -> Self {
        let () = <M as WellFormedRanges>::WELL_FORMED;

        Self {
            source: Arc::new(source),
            media_type: std::marker::PhantomData,
            content_type: Declared,
            etag: None,
            last_modified: None,
            cache_control: None,
            disposition: None,
        }
    }

    /// The media type this representation is sent as, chosen for this request.
    ///
    /// Sent as `Content-Type` on the 200 and the 206 alone: the 304 and the 412
    /// carry none, and the 416 stays the problem document it is.
    ///
    /// A value that is no `type/subtype` with well-formed parameters (RFC 9110
    /// section 8.3.1), or that none of `M`'s [`RANGES`](MediaRanges::RANGES)
    /// admits, has `deliver` answer 500 in place of each response that would
    /// carry it, since the description would otherwise be wrong. A range admits
    /// a type on type and subtype in any case, and, where it states parameters,
    /// only a type carrying each of them.
    #[must_use]
    pub fn content_type(self, value: impl AsRef<str>) -> Served<S, M, Detected> {
        Served {
            source: self.source,
            media_type: self.media_type,
            content_type: Detected(crate::http::media::admitted::<M>(value.as_ref())),
            etag: self.etag,
            last_modified: self.last_modified,
            cache_control: self.cache_control,
            disposition: self.disposition,
        }
    }
}

impl<S: ByteSource, M: MediaType> Served<S, M> {
    /// Answers `parts`, reading only what the answer needs, sent as `M` states.
    ///
    /// # Errors
    ///
    /// Returns the source's own error if the length cannot be read; a tag
    /// that cannot be spelled is answered with a 500 before the source is
    /// asked, so it is never that error. A failed
    /// span read, or a source that stops short of the length it reported
    /// ([`Truncated`](super::source::Truncated)), fails the body stream instead,
    /// since the head has already been sent.
    pub async fn deliver(self, conditions: &Conditions) -> Result<Delivery<M>, S::Error> {
        // Static, so it costs no allocation: `WELL_FORMED` refused a marker
        // `from_static` would not take.
        let () = <M as WellFormed>::WELL_FORMED;
        self.answer(conditions, Some(HeaderValue::from_static(M::MEDIA_TYPE)))
            .await
    }
}

impl<S: ByteSource, M: MediaRanges> Served<S, M, Detected> {
    /// Answers `parts`, reading only what the answer needs, sent as
    /// [`content_type`](Served::content_type) was given.
    ///
    /// # Errors
    ///
    /// Returns the source's own error if the length cannot be read; a tag
    /// that cannot be spelled is answered with a 500 before the source is
    /// asked, so it is never that error. A failed
    /// span read, or a source that stops short of the length it reported
    /// ([`Truncated`](super::source::Truncated)), fails the body stream instead,
    /// since the head has already been sent.
    pub async fn deliver(mut self, conditions: &Conditions) -> Result<Delivery<M>, S::Error> {
        let content_type = self.content_type.0.take();
        self.answer(conditions, content_type).await
    }
}

impl<S: ByteSource, M: MediaRanges, T> Served<S, M, T> {
    /// The validator this representation is known by.
    ///
    /// A resumable download needs a strong tag: without one, `If-Range` cannot
    /// hold and a resume gets the whole representation (section 13.1.5).
    ///
    /// A tag [`ETag::encode`] cannot spell has [`deliver`](Self::deliver)
    /// answer 500, whatever the request's preconditions.
    #[must_use]
    pub fn etag(mut self, etag: ETag) -> Self {
        self.etag = Some(etag);
        self
    }

    /// When the representation last changed.
    ///
    /// The weaker validator, at one-second resolution; section 13.1.3 ranks it
    /// below `ETag`.
    ///
    /// An instant before the Unix epoch or past year 9999 has no HTTP-date
    /// spelling (section 5.6.7), so the `Last-Modified` field is omitted for it.
    #[must_use]
    pub fn last_modified(mut self, at: SystemTime) -> Self {
        self.last_modified = Some(at);
        self
    }

    /// How long this representation may be reused.
    ///
    /// A value that is no field value (RFC 9110 section 5.5) has
    /// [`deliver`](Self::deliver) answer 500 in place of each response that
    /// would carry it: the 200, the 206 and the 304.
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

    /// Section 13.2.2's algorithm, labelling a 200 or 206 `content_type`; one
    /// that cannot be sent is `None`, refused as a field `head` cannot encode.
    async fn answer(
        self,
        conditions: &Conditions,
        content_type: Option<HeaderValue>,
    ) -> Result<Delivery<M>, S::Error> {
        // Encoded once, before any precondition reads the tag: one that cannot
        // be spelled would otherwise be evaluated as no tag at all. Before the
        // source is read too, since no answer it gives changes this one.
        let tag = match self.etag.as_ref().map(ETag::encode) {
            Some(None) => return Ok(Delivery::new(crate::response::headers::refused())),
            Some(Some(tag)) => Some(tag),
            None => None,
        };
        // As it is spelled on the wire, for the three fields that compare it.
        let current = tag.as_ref().and_then(|tag| tag.to_str().ok());

        let complete_length = self.source.complete_length().await?;

        // Section 13.2.2 steps 1 and 2: the lost-update preconditions first.
        if self.precondition_failed(conditions, current) {
            return Ok(Delivery::precondition_failed());
        }

        // Steps 3 and 4: the cache validations.
        if self.unmodified(conditions, current) {
            // Section 13.1.2: a false `If-None-Match` is a 304 for GET and
            // HEAD alone, and a 412 for any other method. Only it can hold
            // here for one, since the date applies to GET and HEAD alone.
            if conditions.method != Method::GET && conditions.method != Method::HEAD {
                return Ok(Delivery::precondition_failed());
            }
            let response = self
                .head(None, complete_length, tag, content_type)
                .unwrap_or_else(crate::response::headers::refused);
            return Ok(Delivery::new(response));
        }

        // Section 14.2: the field is read only where a 200 was owed.
        let requested = spec::read(&conditions.method, &conditions.fields, current);
        let selection = match crate::response::range::select(&requested, complete_length) {
            Ok(selection) => selection,
            Err(rejection) => return Ok(Delivery::new(rejection.into_response())),
        };

        let (first, last) = match selection {
            Selection::Whole(_) => (0, complete_length.saturating_sub(1)),
            Selection::Part { first, last, .. } => (first, last),
        };

        // A refused head is answered as it is: the content goes only under
        // the fields that describe it.
        let Some(mut response) = self.head(Some(selection), complete_length, tag, content_type)
        else {
            return Ok(Delivery::new(crate::response::headers::refused()));
        };

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

    /// The response fields: the 200 or 206 `selection` frames, or the 304
    /// where there is none.
    ///
    /// `tag` is [`etag`](Self::etag) as `deliver` encoded it, moved in so it
    /// is not encoded twice. `None` where a field cannot be encoded, as a 200
    /// or 206 whose `content_type` is `None` cannot: the caller sends the
    /// shared refusal in its place, and never with the content.
    fn head(
        &self,
        selection: Option<Selection>,
        complete_length: u64,
        tag: Option<HeaderValue>,
        content_type: Option<HeaderValue>,
    ) -> Option<Response> {
        let mut response = Response::new(crate::http::body::Body::empty());

        // Section 14.3: `Accept-Ranges` is advertised on every response
        // carrying a representation, which a 304 does not; section 15.4.5 asks
        // a 304 for no metadata beyond its validators and `Cache-Control`, so
        // the name stays here too.
        match selection {
            Some(selection) => write_selection(&mut response, selection).ok()?,
            None => *response.status_mut() = StatusCode::NOT_MODIFIED,
        }
        let fields = response.headers_mut();

        if selection.is_some() {
            fields.insert(header::CONTENT_TYPE, content_type?);
            if let Some(disposition) = &self.disposition {
                if crate::response::headers::write(fields, disposition).is_err() {
                    return None;
                }
            }
        }

        // `ETag` declares no `Vary` and does not repeat, so inserting it is
        // what writing the group would do.
        if let Some(tag) = tag {
            fields.insert(header::ETAG, tag);
        }
        if let Some(value) = self
            .last_modified
            .and_then(crate::http::date::format)
            .and_then(|rendered| HeaderValue::from_str(&rendered).ok())
        {
            fields.insert(header::LAST_MODIFIED, value);
        }
        if let Some(value) = &self.cache_control {
            let Ok(value) = HeaderValue::from_str(value) else {
                return None;
            };
            fields.insert(header::CACHE_CONTROL, value);
        }

        match selection {
            Some(Selection::Part { first, last, .. }) => {
                fields.insert(header::CONTENT_LENGTH, decimal(last - first + 1));
            }
            Some(Selection::Whole(_)) => {
                fields.insert(header::CONTENT_LENGTH, decimal(complete_length));
            }
            None => {}
        }

        Some(response)
    }

    /// Whether section 13.2.2's first two steps refuse the request.
    ///
    /// `If-Unmodified-Since` is ignored when `If-Match` is present (section
    /// 13.1.4).
    fn precondition_failed(&self, conditions: &Conditions, current: Option<&str>) -> bool {
        // Section 13.1.1, the same evaluation an asset makes.
        if let Some(holds) = crate::http::etag::if_match(&conditions.fields, || current) {
            return !holds;
        }

        // Section 13.1.4: ignored unless exactly one HTTP-date and a
        // modification date to compare.
        let (Some(modified), Some(since)) = (
            self.last_modified,
            single_date(&conditions.fields, header::IF_UNMODIFIED_SINCE),
        ) else {
            return false;
        };

        seconds(modified) > seconds(since)
    }

    /// Whether section 13.1's preconditions say the client's copy is current.
    fn unmodified(&self, conditions: &Conditions, current: Option<&str>) -> bool {
        // Section 13.1.3: `If-None-Match` overrides `If-Modified-Since`.
        // Section 13.1.2: `*` names any current representation, which the
        // length `deliver` read proves exists; a tag list needs a tag.
        if let Some(named) = crate::http::etag::if_none_match(&conditions.fields, || current) {
            return named;
        }

        // Section 13.1.3: the date applies to GET and HEAD alone.
        if conditions.method != Method::GET && conditions.method != Method::HEAD {
            return false;
        }

        // Section 13.1.3: ignored unless exactly one HTTP-date, as
        // `If-Unmodified-Since` is.
        let (Some(modified), Some(since)) = (
            self.last_modified,
            single_date(&conditions.fields, header::IF_MODIFIED_SINCE),
        ) else {
            return false;
        };

        // At the field's one-second resolution.
        seconds(modified) <= seconds(since)
    }
}

/// `length` as a `Content-Length` value, in the one allocation the value
/// copies its octets into.
fn decimal(length: u64) -> HeaderValue {
    use std::io::Write;

    // `u64::MAX` has twenty digits, so the write always fits.
    let mut digits = [0; 20];
    let mut rest = &mut digits[..];
    let _ = write!(rest, "{length}");
    let unwritten = rest.len();
    let written = digits.len() - unwritten;
    // Digits are always a field value; the fallback is the same value, built
    // the costlier way.
    HeaderValue::from_bytes(&digits[..written]).unwrap_or_else(|_| HeaderValue::from(length))
}

/// The HTTP-date `name` carries, or `None` unless it is sent on exactly one
/// line holding one.
fn single_date(
    fields: &crate::http::HeaderMap,
    name: crate::http::HeaderName,
) -> Option<SystemTime> {
    let mut lines = fields.get_all(name).iter();
    let (Some(line), None) = (lines.next(), lines.next()) else {
        return None;
    };
    line.to_str().ok().and_then(crate::http::date::parse)
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
pub struct Delivery<M: MediaRanges> {
    response: Response,
    media_type: std::marker::PhantomData<fn() -> M>,
}

// By hand, so it is not bounded on the marker, which a derive would ask to be
// `Debug` too.
impl<M: MediaRanges> std::fmt::Debug for Delivery<M> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Delivery")
            .field("response", &self.response)
            .finish_non_exhaustive()
    }
}

impl<M: MediaRanges> Delivery<M> {
    fn new(response: Response) -> Self {
        Self {
            response,
            media_type: std::marker::PhantomData,
        }
    }

    /// The 412 a failed precondition answers, which sends no content.
    fn precondition_failed() -> Self {
        Self::new(bodiless(StatusCode::PRECONDITION_FAILED))
    }

    /// The status this delivery will send.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.response.status()
    }
}

impl<M: MediaRanges> IntoResponse for Delivery<M> {
    fn into_response(self) -> Response {
        self.response
    }
}

impl<M: MediaRanges> crate::response::Responses for Delivery<M> {
    fn responses(registry: &mut crate::schema::registry::Registry) -> kynos_openapi::Responses {
        let () = <M as WellFormedRanges>::WELL_FORMED;
        delivery::responses(registry, M::RANGES)
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
    /// The fields [`READ`] names, read by `spec::read` and the precondition
    /// checks.
    pub(super) fields: crate::http::HeaderMap,
}

/// The fields a ranged delivery reads, which [`Conditions`] keeps.
const READ: [crate::http::HeaderName; 6] = [
    header::RANGE,
    header::IF_RANGE,
    header::IF_MATCH,
    header::IF_UNMODIFIED_SINCE,
    header::IF_NONE_MATCH,
    header::IF_MODIFIED_SINCE,
];

impl<C: Sync> crate::extract::FromRequestParts<C> for Conditions {
    type Rejection = std::convert::Infallible;

    /// Infallible: every unusable value among the six is ignored or fails its
    /// condition (RFC 9110 sections 13.1, 14.2).
    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        // Only the six fields, every line of each: a request carrying none of
        // them costs no allocation, and the rest of the head is not copied.
        let mut fields = crate::http::HeaderMap::new();
        for name in READ {
            for value in parts.headers.get_all(&name) {
                if fields.is_empty() {
                    // A first `append` would reserve six entries; a request
                    // usually sends one or two of these, as `Range` and
                    // `If-Range` are. Counting them first costs a second
                    // lookup of each.
                    fields.reserve(2);
                }
                fields.append(name.clone(), value.clone());
            }
        }

        Ok(Self {
            method: parts.method.clone(),
            fields,
        })
    }
}

impl crate::extract::describe::Describe for Conditions {
    fn describe(operation: &mut crate::router::operation::OperationCx<'_>) {
        operation.add_parameter(crate::response::range::parameter());
        operation.add_parameter(delivery::conditional_parameter());

        for precondition in [
            Precondition::Match,
            Precondition::UnmodifiedSince,
            Precondition::NoneMatch,
            Precondition::ModifiedSince,
        ] {
            operation.add_parameter(precondition.parameter());
        }
    }
}
