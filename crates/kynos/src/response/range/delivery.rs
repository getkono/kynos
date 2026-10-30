//! How a ranged delivery is described: the statuses, fields and
//! preconditions a [`Served`](super::served::Served) delivery and a ranged
//! asset share, and the `Accept-Ranges` advertisement every ranged response
//! type adds to its body's, so each describes them in the same words.

use kynos_openapi::{
    Header, Parameter, Response, Schema, StatusPattern, model::schema::types::SchemaType,
};

use crate::{
    error::rejection::RangeRejection,
    extract::params::header::HeaderParams,
    response::{
        Responses,
        codec::unconstrained,
        disposition::ContentDisposition,
        headers::declare_on_each,
        range::{headers, headers::AcceptRanges, rangeable::Rangeable},
    },
    schema::registry::Registry,
};

/// The `If-Range` precondition on applying the `Range` field.
///
/// Declared only where a validator exists to evaluate it, which excludes
/// [`Range<T>`](super::Range).
#[must_use]
pub(crate) fn conditional_parameter() -> Parameter {
    Parameter::header("If-Range", Schema::of_type(SchemaType::String)).with_description(
        "The entity tag the client's partial copy came from, per RFC 9110 section 13.1.5. The \
         `Range` is honoured only if it matches this representation under the strong \
         comparison; otherwise the whole representation is sent.",
    )
}

/// A precondition field a validated delivery reads besides `If-Range`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Precondition {
    /// `If-Match`, section 13.1.1.
    Match,
    /// `If-Unmodified-Since`, section 13.1.4.
    UnmodifiedSince,
    /// `If-None-Match`, section 13.1.2.
    NoneMatch,
    /// `If-Modified-Since`, section 13.1.3.
    ModifiedSince,
}

impl Precondition {
    /// The header parameter declaring this field.
    pub(crate) fn parameter(self) -> Parameter {
        let (name, description) = match self {
            Self::Match => (
                "If-Match",
                "The entity tag the client's copy was taken from, per RFC 9110 section 13.1.1",
            ),
            Self::UnmodifiedSince => (
                "If-Unmodified-Since",
                "The date the client's copy carries, refused if the representation changed since, \
                 per RFC 9110 section 13.1.4",
            ),
            Self::NoneMatch => (
                "If-None-Match",
                "The entity tag the client already holds, per RFC 9110 section 13.1.2",
            ),
            Self::ModifiedSince => (
                "If-Modified-Since",
                "The date the client's copy carries, per RFC 9110 section 13.1.3",
            ),
        };

        Parameter::header(name, Schema::of_type(SchemaType::String)).with_description(description)
    }
}

/// A 200 or 206 carrying `media_type`, its schema unconstrained, as every
/// binary codec describes its bytes.
pub(crate) fn content_response(description: &str, media_type: &str) -> Response {
    Response::with_content(description, media_type, unconstrained())
}

/// The 304 a validated delivery sends when the client's copy is current.
pub(crate) fn not_modified_response() -> Response {
    Response::new("the client's copy is current")
}

/// `T`'s own responses, each advertising `Accept-Ranges`, and that
/// advertisement for the 206 a ranged type adds beside them.
pub(crate) fn advertised<T: Rangeable>(
    registry: &mut Registry,
) -> (
    kynos_openapi::Responses,
    kynos_openapi::Map<kynos_openapi::RefOr<Header>>,
) {
    let advertised = AcceptRanges::response_headers(registry);
    let mut responses = T::responses(registry);
    declare_on_each(&mut responses, &advertised, true);
    (responses, advertised)
}

/// The statuses and fields a [`Served`](super::served::Served) delivery can
/// produce.
///
/// 200, 206, 304, 412 and [`RangeRejection`]'s 416; no 400, since section 14.2
/// ignores an unusable `Range`. The fields the builder adds only on request,
/// `Cache-Control` and `Content-Disposition`, are declared optional.
#[must_use]
pub(crate) fn responses(registry: &mut Registry, media_type: &str) -> kynos_openapi::Responses {
    let string = || Header::new(Schema::of_type(SchemaType::String));
    // The type's own description, optional here: a delivery names a file only
    // where its builder asked for one. A drift away from an inline header must
    // fail loudly rather than declare an undescribed field.
    let mut disposition = ContentDisposition::response_headers(registry)
        .into_values()
        .find_map(|header| header.as_item().cloned())
        .expect("ContentDisposition declares its header inline");
    disposition.required = None;

    let mut responses = kynos_openapi::Responses::new()
        .with(
            200,
            content_response("the whole representation", media_type),
        )
        .with(
            206,
            content_response("the part the request asked for", media_type),
        )
        .with(304, not_modified_response())
        .with(
            412,
            Response::new("the representation is not the one the client's copy came from"),
        );

    for (status, name, header) in [
        (200, "Accept-Ranges", string()),
        (206, "Accept-Ranges", string()),
        (200, "ETag", string()),
        (206, "ETag", string()),
        (304, "ETag", string()),
        (200, "Last-Modified", string()),
        (206, "Last-Modified", string()),
        (304, "Last-Modified", string()),
        // Section 15.4.5: a 304 owes `Cache-Control` wherever the 200 carries
        // it, and no other metadata, so `Content-Disposition` stops at 206.
        (200, "Cache-Control", string()),
        (206, "Cache-Control", string()),
        (304, "Cache-Control", string()),
        (200, "Content-Disposition", disposition.clone()),
        (206, "Content-Disposition", disposition),
        (
            206,
            "Content-Range",
            headers::ContentRange::satisfied_header(),
        ),
    ] {
        if let Some(kynos_openapi::RefOr::Item(response)) = responses
            .responses
            .get_mut(&StatusPattern::Code(status).to_string())
        {
            response
                .headers
                .insert(name.to_owned(), kynos_openapi::RefOr::Item(header));
        }
    }

    responses
        .responses
        .extend(<RangeRejection as Responses>::responses(registry).responses);

    responses
}
