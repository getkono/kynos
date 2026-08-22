//! Answering a byte range against a file, and describing that it can be.
//!
//! Both asset modes end here. An embedded file is a `&'static [u8]` and a
//! served one has just been read, so by this point the representation is octets
//! of a known length whichever way it arrived — which is exactly what RFC 9110
//! section 14.1.2 defines a byte range over.
//!
//! Nothing here decides what a range *means*. The reader, the satisfiability
//! rule and both response fields come from
//! [`response::range`](crate::response::range); this module chooses only how
//! the answer is written and what the operation says about it.
//!
//! # An asset is where `If-Range` becomes real
//!
//! Section 13.1.5 makes `If-Range` a precondition on applying the field, and
//! evaluating it needs a validator. A handler's `Ranged<T>` has none, so
//! `response::range` ignores the condition and sends the whole representation.
//! An asset has one: [`assets!`](crate::assets) mints a strong entity tag from
//! the file's contents. So the tag goes to the reader, the strong comparison
//! decides, and a client resuming a download it started before a deployment
//! gets the new file whole rather than a part spliced into a stale copy.

use bytes::Bytes;

use crate::{
    error::rejection::RangeRejection,
    extract::params::header::HeaderParams,
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

/// The whole representation, the part a `Range` asked for, or a 416.
///
/// For octets already in hand. A sender that knows the length without holding
/// the bytes — a file on disk — calls [`range::select`] itself and reaches
/// [`assembled`] with only the part it read.
pub(super) fn respond<H: HeaderParams>(
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
pub(super) fn assembled<H: HeaderParams>(
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

    // Section 14.3: the advertisement rides on every representation this
    // operation serves, which is what tells a client a resumable download
    // exists at all.
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
///
/// The rejection writes both halves, so the `unsatisfied-range` grammar is
/// stated in one place and the asset server restates none of it.
pub(super) fn unsatisfiable(rejection: RangeRejection) -> Response {
    rejection.into_response()
}

/// Declares the two statuses, two parameters and one field a ranged file adds.
///
/// Called with the 200 and the 304 already declared, because the `Content-Range`
/// on a 206 and the `Accept-Ranges` on both successes are attached per status —
/// section 14.4 gives the first no meaning anywhere else, and a 304 is not a
/// representation to advertise a range of.
pub(super) fn describe(operation: &mut OperationCx<'_>, media_type: &str) {
    let partial = kynos_openapi::Response::with_content(
        "the requested part of the file",
        media_type,
        // The same unconstrained object the 200 carries: a part of a file has
        // no more of a JSON Schema than the file does.
        kynos_openapi::MediaType::new(kynos_openapi::Schema::Object(Box::default())),
    );
    operation.add_responses(kynos_openapi::Responses::new().with(206, partial));

    // The 416 comes from the rejection that produces it, so the problem shape
    // and the `unsatisfied-range` grammar are declared once for the whole
    // framework rather than restated here.
    let unsatisfiable =
        <RangeRejection as crate::response::Responses>::responses(operation.registry());
    operation.add_responses(unsatisfiable);

    operation.add_parameter(range::parameter());
    operation.add_parameter(range::conditional_parameter());

    for status in [200, 206] {
        for (name, header) in AcceptRanges::response_headers(operation.registry()) {
            if let kynos_openapi::RefOr::Item(header) = header {
                operation.add_response_header(
                    kynos_openapi::StatusPattern::Code(status),
                    name,
                    header,
                );
            }
        }
    }

    operation.add_response_header(
        kynos_openapi::StatusPattern::Code(206),
        "Content-Range",
        ContentRange::satisfied_header(),
    );
}
