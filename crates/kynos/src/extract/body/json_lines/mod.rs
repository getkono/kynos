//! The streamed JSON codecs: newline-delimited, and RFC 7464 text sequences.
//!
//! Both are *sequential* media types, described by OpenAPI 3.2's `itemSchema`,
//! so this module needs `openapi32` as well as `json`. [`records`] is the
//! decoder a request body is read with.

pub mod records;

#[cfg(test)]
mod tests;

use kynos_openapi::model::body::mime_names;

use crate::{
    error::rejection::BodyRejection,
    extract::{
        FromRequest,
        body::json_lines::records::{Framing, Records},
        describe::{Describe, RequestContent},
    },
    http::Request,
    router::operation::OperationCx,
    schema::{Schema, registry::Registry},
};

/// The NDJSON media type, shared by decoding, description and the response half.
pub(crate) const LINES_MEDIA_TYPE: &str = mime_names::APPLICATION_NDJSON;

/// The JSON text sequence media type, shared as [`LINES_MEDIA_TYPE`] is.
pub(crate) const SEQUENCE_MEDIA_TYPE: &str = mime_names::APPLICATION_JSON_SEQ;

/// A newline-delimited JSON body (`application/x-ndjson`).
///
/// Requires both `json` and `openapi32`; the latter supplies the `itemSchema`
/// needed to describe each streamed value.
///
/// One type, both directions. As a response, `items` is any stream of
/// serializable values and each is written as one line. As a request, `items`
/// is [`Records<T>`], which decodes the body one line at a time and holds each
/// record to the bounds `T`'s schema declares, so extracting one requires
/// `T: Schema` as well as `T: DeserializeOwned`.
///
/// ```no_run
/// # #[cfg(all(feature = "json", feature = "openapi32"))]
/// # {
/// use kynos::{
///     error::rejection::BodyRejection,
///     extract::body::json_lines::{JsonLines, records::Records},
///     response::status::NoContent,
/// };
///
/// #[derive(kynos::Schema, serde::Deserialize)]
/// struct Reading {
///     value: f64,
/// }
///
/// async fn ingest(
///     JsonLines { mut items }: JsonLines<Records<Reading>>,
/// ) -> Result<NoContent, BodyRejection> {
///     while let Some(reading) = items.next().await {
///         drop(reading?.value);
///     }
///     Ok(NoContent)
/// }
///
/// fn lines<S>(items: S) -> JsonLines<S> {
///     JsonLines { items }
/// }
/// # }
/// ```
#[derive(Debug)]
pub struct JsonLines<S> {
    /// The stream of items.
    pub items: S,
}

/// An RFC 7464 JSON text sequence body (`application/json-seq`).
///
/// Requires both `json` and `openapi32`; the latter supplies the `itemSchema`
/// needed to describe each streamed value.
///
/// The same items as [`JsonLines`] under a different framing. RFC 7464's
/// separator is a *prefix*, so a record is complete only once the next one
/// arrives or the body ends; in exchange a record may contain newlines.
///
/// ```no_run
/// # #[cfg(all(feature = "json", feature = "openapi32"))]
/// # {
/// use kynos::{
///     error::rejection::BodyRejection,
///     extract::body::json_lines::{JsonSeq, records::Records},
/// };
///
/// #[derive(kynos::Schema, serde::Deserialize)]
/// struct Reading {
///     value: f64,
/// }
///
/// async fn ingest(
///     JsonSeq { items }: JsonSeq<Records<Reading>>,
/// ) -> Result<String, BodyRejection> {
///     Ok(format!("{} readings", items.read_all().await?.len()))
/// }
///
/// fn sequence<S>(items: S) -> JsonSeq<S> {
///     JsonSeq { items }
/// }
/// # }
/// ```
#[derive(Debug)]
pub struct JsonSeq<S> {
    /// The stream of items.
    pub items: S,
}

impl<C: Sync, T: serde::de::DeserializeOwned + Schema> FromRequest<C> for JsonLines<Records<T>> {
    type Rejection = BodyRejection;

    async fn from_request(request: Request, _context: &C) -> Result<Self, Self::Rejection> {
        Records::new(request, LINES_MEDIA_TYPE, Framing::Lines).map(|items| Self { items })
    }
}

impl<T: Schema> Describe for JsonLines<Records<T>> {
    fn describe(operation: &mut OperationCx<'_>) {
        let body = <Self as RequestContent>::request_body(operation.registry());
        operation.set_request_body(body);
    }
}

impl<T: Schema> RequestContent for JsonLines<Records<T>> {
    fn media_types() -> Vec<&'static str> {
        vec![LINES_MEDIA_TYPE]
    }

    // `itemSchema` alone, matching what the response half emits.
    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        kynos_openapi::RequestBody::new(
            LINES_MEDIA_TYPE,
            kynos_openapi::MediaType::sequential(registry.resolve::<T>()),
        )
    }
}

impl<C: Sync, T: serde::de::DeserializeOwned + Schema> FromRequest<C> for JsonSeq<Records<T>> {
    type Rejection = BodyRejection;

    async fn from_request(request: Request, _context: &C) -> Result<Self, Self::Rejection> {
        Records::new(request, SEQUENCE_MEDIA_TYPE, Framing::Sequence).map(|items| Self { items })
    }
}

impl<T: Schema> Describe for JsonSeq<Records<T>> {
    fn describe(operation: &mut OperationCx<'_>) {
        let body = <Self as RequestContent>::request_body(operation.registry());
        operation.set_request_body(body);
    }
}

impl<T: Schema> RequestContent for JsonSeq<Records<T>> {
    fn media_types() -> Vec<&'static str> {
        vec![SEQUENCE_MEDIA_TYPE]
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        kynos_openapi::RequestBody::new(
            SEQUENCE_MEDIA_TYPE,
            kynos_openapi::MediaType::sequential(registry.resolve::<T>()),
        )
    }
}
