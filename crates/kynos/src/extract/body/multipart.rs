//! The `multipart/form-data` body codec.

use std::collections::BTreeMap;

use bytes::Bytes;
use http_body_util::BodyExt;

use crate::{
    error::rejection::BodyRejection,
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
    },
    http::{HeaderMap, Request, header},
    router::operation::OperationCx,
    schema::{Schema, registry::Registry},
};

/// A `multipart/form-data` request body with declared fields.
///
/// `T` derives `Schema`, and each field becomes a part with its own `Encoding`.
/// The same wrapper may be returned as a response, preserving the declared
/// field names, per-part media types, and encodings in both directions.
/// There is no dynamic-field iterator: a handler that accepts arbitrary part
/// names cannot describe them. For a variable number of uploads, declare one
/// field of type `Vec<FilePart>`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MultipartForm<T>(pub T);

/// One spelling, read by both halves: what is decoded and what is described.
const MEDIA_TYPE: &str = "multipart/form-data";

/// One uploaded file within a [`MultipartForm`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilePart {
    /// The client-supplied file name, if any.
    pub file_name: Option<String>,
    /// The declared media type of this part.
    pub content_type: Option<String>,
    /// The part's bytes.
    pub bytes: bytes::Bytes,
}

/// A part's bytes are raw binary, which sits outside JSON Schema's `type`
/// exactly as a raw binary message body does.
///
/// The part's media type is the Encoding Object's to state, and a
/// `contentMediaType` here would contradict it — which the specification says
/// is ignored. So the schema is the empty one, and every part-level fact is
/// carried where a consumer will actually read it. See `docs/schema.md`.
impl Schema for FilePart {
    fn schema(_registry: &mut Registry) -> kynos_openapi::Schema {
        kynos_openapi::Schema::Object(Box::default())
    }
}

/// One part of a `multipart/form-data` body, with the field name it carries.
///
/// A part always has a name: RFC 7578 requires every part to carry a
/// `Content-Disposition` naming the form field it belongs to, so a part without
/// one belongs to no declared field and the body is malformed.
///
/// This is the currency both directions trade in — [`FromMultipart`] receives
/// these and [`IntoMultipart`](crate::response::codec::multipart::IntoMultipart)
/// produces them — which is what makes the field names, media types and
/// encodings the same in both.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Part {
    /// The form field this part carries.
    pub name: String,
    /// The client-supplied file name, if any.
    pub file_name: Option<String>,
    /// The declared media type of this part.
    pub content_type: Option<String>,
    /// The part's bytes.
    pub bytes: Bytes,
}

/// A file part is one part with its name forgotten: the name is the field it
/// filled, which the declaring type already records.
impl From<Part> for FilePart {
    fn from(part: Part) -> Self {
        Self {
            file_name: part.file_name,
            content_type: part.content_type,
            bytes: part.bytes,
        }
    }
}

/// How a declared type is built from a decoded `multipart/form-data` body.
///
/// A multipart body is decoded part by part rather than through a
/// `Deserializer`, so this is the trait that says how parts become a value —
/// the role `DeserializeOwned` plays for [`Json`](super::json::Json) and
/// [`Form`](super::form::Form). `#[derive(MultipartForm)]` writes it, along
/// with its writing counterpart, from the field declarations.
///
/// The parts arrive in the order the client sent them, and a part naming no
/// declared field is left for the implementation to ignore or refuse.
///
/// ```
/// use kynos::{
///     error::rejection::BodyRejection,
///     extract::body::multipart::{FilePart, FromMultipart, Part},
/// };
///
/// struct Avatar(FilePart);
///
/// impl FromMultipart for Avatar {
///     fn from_parts(parts: Vec<Part>) -> Result<Self, BodyRejection> {
///         parts
///             .into_iter()
///             .find(|part| part.name == "avatar")
///             .map(|part| Self(FilePart::from(part)))
///             .ok_or_else(|| BodyRejection::Syntax {
///                 detail: "no `avatar` part".to_owned(),
///             })
///     }
/// }
/// ```
pub trait FromMultipart: Sized {
    /// Builds the value from every part the body carried, in arrival order.
    ///
    /// # Errors
    ///
    /// Returns the rejection describing which part was missing, repeated or
    /// unreadable.
    fn from_parts(parts: Vec<Part>) -> Result<Self, BodyRejection>;
}

/// How one declared field is built from the part carrying it.
///
/// Multipart's answer to the [`FromStr`](std::str::FromStr) that
/// [`params`](crate::extract::params) decodes a parameter through: a part is
/// bytes plus a media type rather than text, so the conversion a Rust program
/// already has for text is not the one that applies.
///
/// Implemented for [`FilePart`], `String` and [`Bytes`], which are the three
/// shapes a form field takes. `#[derive(MultipartForm)]` reads an `Option<T>`
/// field as an optional part and a `Vec<T>` field as a repeated one, so an
/// implementation here only ever answers for a single part.
pub trait FromPart: Sized {
    /// Builds the field from one part.
    ///
    /// # Errors
    ///
    /// Returns the rejection describing why the part could not become this
    /// field, keyed by the part's own name.
    fn from_part(part: Part) -> Result<Self, BodyRejection>;
}

/// The 422 a part that cannot become its field produces, keyed by the field's
/// name as a JSON Pointer into the body.
fn malformed_part(name: &str, detail: &str) -> BodyRejection {
    BodyRejection::Schema {
        failures: BTreeMap::from([(format!("/{name}"), detail.to_owned())]),
    }
}

impl FromPart for FilePart {
    fn from_part(part: Part) -> Result<Self, BodyRejection> {
        Ok(Self::from(part))
    }
}

impl FromPart for Bytes {
    fn from_part(part: Part) -> Result<Self, BodyRejection> {
        Ok(part.bytes)
    }
}

/// Every text format Kynos decodes is UTF-8, so a part that is not is a part
/// this field cannot hold rather than one to reinterpret in another charset.
impl FromPart for String {
    fn from_part(part: Part) -> Result<Self, BodyRejection> {
        Self::from_utf8(part.bytes.to_vec())
            .map_err(|_| malformed_part(&part.name, "the part is not valid UTF-8"))
    }
}

/// The delimiter the request declares, or the rejection saying why there is
/// none to read the body with.
///
/// A `Content-Type` naming anything but `multipart/form-data` is the 415 every
/// codec here raises. One naming it without a `boundary` is different: the
/// media type is accepted and RFC 2046 delimits the parts with that parameter,
/// so what arrived is a body no parser can find the parts in.
fn boundary(headers: &HeaderMap) -> Result<String, BodyRejection> {
    let declared = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());

    match declared.map(multer::parse_boundary) {
        Some(Ok(boundary)) => Ok(boundary),
        Some(Err(multer::Error::NoBoundary)) => Err(BodyRejection::Syntax {
            detail: format!("`{MEDIA_TYPE}` declares no `boundary` parameter"),
        }),
        _ => Err(super::unsupported_media_type(headers)),
    }
}

/// Every way the parser can fail is a body that is not the one the client meant
/// to send, which is the same 400 a transport failure part-way through is.
fn malformed_body(error: &multer::Error) -> BodyRejection {
    BodyRejection::Syntax {
        detail: error.to_string(),
    }
}

impl<C: Sync, T: FromMultipart + Send> FromRequest<C> for MultipartForm<T> {
    type Rejection = BodyRejection;

    async fn from_request(request: Request, _context: &C) -> Result<Self, Self::Rejection> {
        let boundary = boundary(request.headers())?;
        let mut fields = multer::Multipart::new(request.into_body().into_data_stream(), boundary);

        // Every part is read to completion before `T` is built, for the reason
        // the JSON codec serializes before it commits a status: a decision made
        // half-way through a body has already spent the response it would need
        // to report the rest.
        let mut parts = Vec::new();
        while let Some(field) = fields.next_field().await.map_err(|e| malformed_body(&e))? {
            let Some(name) = field.name().map(str::to_owned) else {
                return Err(BodyRejection::Syntax {
                    detail: "a part declares no field name in its `Content-Disposition`".to_owned(),
                });
            };
            let file_name = field.file_name().map(str::to_owned);
            let content_type = field.content_type().map(ToString::to_string);
            let bytes = field.bytes().await.map_err(|e| malformed_body(&e))?;

            parts.push(Part {
                name,
                file_name,
                content_type,
                bytes,
            });
        }

        T::from_parts(parts).map(Self)
    }
}

impl<T: Schema> Describe for MultipartForm<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let body = <Self as RequestContent>::request_body(operation.registry());
        operation.set_request_body(body);
    }
}

impl<T: Schema> RequestContent for MultipartForm<T> {
    fn media_types() -> Vec<&'static str> {
        vec![MEDIA_TYPE]
    }

    // No Encoding Object is written, because the specification's default for a
    // property is derived from that property's schema -- `application/json` for
    // an object, `application/octet-stream` for anything typeless, `text/plain`
    // otherwise -- and those are exactly the values Kynos would emit. A
    // `FilePart` is the typeless case and a `String` field the last one, so
    // stating them would repeat the schema rather than add to it. An encoding
    // that departs from the default is a per-field decision, and the schema
    // reaching this point may be a `$ref` with no fields left to read.
    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        kynos_openapi::RequestBody::new(
            MEDIA_TYPE,
            kynos_openapi::MediaType::new(registry.resolve::<T>()),
        )
    }
}
