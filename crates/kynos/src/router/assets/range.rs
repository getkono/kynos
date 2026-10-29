//! Answering a byte range against a file, and describing that it can be.
//!
//! Range semantics come from [`response::range`](crate::response::range); this
//! module only writes the answer and describes it. Unlike a handler's
//! `Ranged<T>`, an asset has an entity tag, so `If-Range` (RFC 9110 section
//! 13.1.5) is evaluated rather than ignored.

use bytes::Bytes;

use crate::{
    error::rejection::RangeRejection,
    extract::params::header::{EncodeHeaders, HeaderParams},
    http::{HeaderValue, Response, StatusCode, header},
    response::{
        IntoResponse,
        range::{
            self, Selection,
            headers::{AcceptRanges, ContentRange},
            rangeable::clamped,
            spec::{Ignored, Spec},
        },
    },
    router::operation::OperationCx,
};

/// Section 13.1.1's 412, if the request's `If-Match` fails for a file tagged
/// `current`.
///
/// Called first, as section 13.2.2 orders. `If-Unmodified-Since` is ignored,
/// since no `Last-Modified` is sent (section 13.1.4).
pub(super) fn precondition_failed(
    fields: &crate::http::HeaderMap,
    current: Option<&str>,
) -> Option<Response> {
    if crate::http::etag::if_match(fields, || current)? {
        return None;
    }

    Some(crate::http::body::bodiless(StatusCode::PRECONDITION_FAILED))
}

/// The whole representation, the part a `Range` asked for, or a 416.
///
/// For octets already in hand; a file on disk calls [`range::select`] and
/// [`assembled`] itself.
pub(super) fn respond<H: EncodeHeaders>(
    octets: Bytes,
    media_type: &str,
    headers: &H,
    requested: &Result<Vec<Spec>, Ignored>,
) -> Response {
    let complete_length = u64::try_from(octets.len()).unwrap_or(u64::MAX);

    let selection = match range::select(requested, complete_length) {
        Ok(selection) => selection,
        // Section 15.5.17's 416, with the `Content-Range` it owes.
        Err(rejection) => return rejection.into_response(),
    };

    let body = match selection {
        Selection::Whole(_) => octets,
        Selection::Part { first, last, .. } => clamped(&octets, first, last),
    };

    assembled(
        crate::http::body::Body::from_bytes(body),
        selection,
        media_type,
        headers,
    )
}

/// The response carrying `body`, which is whatever `selection` said to send.
///
/// A body rather than octets, so a file on disk can hand over one that reads
/// a span at a time once polled; its exact size hint is what frames it.
pub(super) fn assembled<H: EncodeHeaders>(
    body: crate::http::body::Body,
    selection: Selection,
    media_type: &str,
    headers: &H,
) -> Response {
    let mut response = Response::new(body);

    if let Ok(value) = HeaderValue::from_str(media_type) {
        response.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    let written = crate::response::headers::write(response.headers_mut(), headers)
        .and_then(|()| range::write_selection(&mut response, selection));
    if written.is_err() {
        return crate::response::headers::refused();
    }

    response
}

/// Declares the two statuses, two parameters and two fields a ranged file adds.
///
/// Called with the 200 and the 304 already declared; fields are attached to
/// the 200 and 206 only.
pub(super) fn describe(operation: &mut OperationCx<'_>, media_type: &str) {
    let partial = range::delivery::content_response("the requested part of the file", media_type);
    operation.add_responses(&kynos_openapi::Responses::new().with(206, partial));

    // Declared by the rejection that produces it.
    let unsatisfiable =
        <RangeRejection as crate::response::Responses>::responses(operation.registry());
    operation.add_responses(&unsatisfiable);

    operation.add_parameter(range::parameter());
    operation.add_parameter(range::delivery::conditional_parameter());

    for status in [200, 206] {
        for (name, header) in AcceptRanges::response_headers(operation.registry()) {
            if let kynos_openapi::RefOr::Item(header) = header {
                operation.add_response_header(
                    kynos_openapi::StatusPattern::Code(status),
                    name,
                    &header,
                );
            }
        }
    }

    operation.add_response_header(
        kynos_openapi::StatusPattern::Code(206),
        "Content-Range",
        &ContentRange::satisfied_header(),
    );
}
