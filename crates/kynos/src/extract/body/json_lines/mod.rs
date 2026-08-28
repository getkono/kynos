//! The streamed JSON codecs: newline-delimited, and RFC 7464 text sequences.
//!
//! Both are *sequential* media types: the body repeats one JSON value rather
//! than being one. That is what OpenAPI 3.2's `itemSchema` describes, and why
//! this module is gated on it as well as on `json`.
//!
//! Here are the two codec types and the six trait halves that read and describe
//! them. [`records`] is the decoder they read *with*, and it is a module of its
//! own because framing bytes into records changes for reasons a media type
//! spelling does not.

pub mod records;

#[cfg(test)]
mod tests;

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

/// One spelling, read by every half: what is decoded, what is described, and
/// what the responding half of this codec sends.
pub(crate) const LINES_MEDIA_TYPE: &str = "application/x-ndjson";

/// One spelling, read by every half, as [`LINES_MEDIA_TYPE`] is.
pub(crate) const SEQUENCE_MEDIA_TYPE: &str = "application/json-seq";

/// A newline-delimited JSON body (`application/x-ndjson`).
///
/// Requires both `json` and `openapi32`; the latter supplies the `itemSchema`
/// needed to describe each streamed value.
///
/// One type, both directions. As a response, `items` is any stream of
/// serializable values and each is written as one line. As a request, `items`
/// is [`Records<T>`], which decodes the body one line at a time.
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
/// #[derive(serde::Deserialize)]
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
/// The same items as [`JsonLines`] under a different framing, and the framing
/// is not a detail. RFC 7464's separator is a *prefix*, so a record is known
/// complete only once the next separator arrives or the body ends: the last
/// record of a `JsonSeq` lags where a `JsonLines` record does not. What that
/// buys is a record that may itself contain newlines — a pretty-printed JSON
/// value is one record here and cannot be carried by NDJSON at all.
///
/// ```no_run
/// # #[cfg(all(feature = "json", feature = "openapi32"))]
/// # {
/// use kynos::{
///     error::rejection::BodyRejection,
///     extract::body::json_lines::{JsonSeq, records::Records},
/// };
///
/// #[derive(serde::Deserialize)]
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

impl<C: Sync, T: serde::de::DeserializeOwned> FromRequest<C> for JsonLines<Records<T>> {
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

    // `itemSchema` alone, and no `schema`. The specification permits both and
    // says so is unlikely to help, and an array `schema` here would contradict
    // what the response half emits for the same media type.
    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        kynos_openapi::RequestBody::new(
            LINES_MEDIA_TYPE,
            kynos_openapi::MediaType::sequential(registry.resolve::<T>()),
        )
    }
}

impl<C: Sync, T: serde::de::DeserializeOwned> FromRequest<C> for JsonSeq<Records<T>> {
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

    // The framing differs from JSON Lines and the described item does not: both
    // repeat one JSON value, which is what a sequential media type is.
    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        kynos_openapi::RequestBody::new(
            SEQUENCE_MEDIA_TYPE,
            kynos_openapi::MediaType::sequential(registry.resolve::<T>()),
        )
    }
}
