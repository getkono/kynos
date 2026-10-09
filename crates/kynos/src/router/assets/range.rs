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

    let mut response = Response::new(crate::http::body::Body::empty());
    *response.status_mut() = StatusCode::PRECONDITION_FAILED;
    Some(response)
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
        Err(rejection) => return unsatisfiable(rejection),
    };

    let body = match selection {
        Selection::Whole(_) => octets,
        Selection::Part { first, last, .. } => clamped(&octets, first, last),
    };

    assembled(body, selection, media_type, headers)
}

/// The response carrying `body`, which is whatever `selection` said to send.
pub(super) fn assembled<H: EncodeHeaders>(
    body: Bytes,
    selection: Selection,
    media_type: &str,
    headers: &H,
) -> Response {
    let mut response = Response::new(crate::http::body::Body::from_bytes(body));

    if let Ok(value) = HeaderValue::from_str(media_type) {
        response.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    crate::extract::params::header::write(response.headers_mut(), headers);

    // Section 14.3: advertised on every representation served.
    crate::extract::params::header::write(response.headers_mut(), &AcceptRanges);

    if let Selection::Part {
        first,
        last,
        complete_length,
    } = selection
    {
        *response.status_mut() = StatusCode::PARTIAL_CONTENT;
        crate::extract::params::header::write(
            response.headers_mut(),
            &ContentRange::Satisfied {
                first,
                last,
                complete_length,
            },
        );
    }

    response
}

/// Section 15.5.17's 416, with the `Content-Range` it owes.
pub(super) fn unsatisfiable(rejection: RangeRejection) -> Response {
    rejection.into_response()
}

/// Declares the two statuses, two parameters and one field a ranged file adds.
///
/// Called with the 200 and the 304 already declared; fields are attached to
/// the 200 and 206 only.
pub(super) fn describe(operation: &mut OperationCx<'_>, media_type: &str) {
    let partial = kynos_openapi::Response::with_content(
        "the requested part of the file",
        media_type,
        kynos_openapi::MediaType::new(kynos_openapi::Schema::Object(Box::default())),
    );
    operation.add_responses(&kynos_openapi::Responses::new().with(206, partial));

    // Declared by the rejection that produces it.
    let unsatisfiable =
        <RangeRejection as crate::response::Responses>::responses(operation.registry());
    operation.add_responses(&unsatisfiable);

    operation.add_parameter(range::parameter());
    operation.add_parameter(range::conditional_parameter());

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
