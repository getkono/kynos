//! The `multipart/form-data` body codec.

pub(crate) mod disposition;

use std::{
    any::{Any, TypeId},
    collections::BTreeMap,
};

use bytes::Bytes;
use http_body_util::BodyExt;
use kynos_openapi::model::body::mime_names;

use crate::{
    error::rejection::BodyRejection,
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
    },
    http::{HeaderMap, Parts, body::Body, header},
    router::operation::OperationCx,
    schema::{ParamValue, Schema, constraints::Pointer, registry::Registry},
};

/// A `multipart/form-data` request body with declared fields.
///
/// `T` derives `Schema` and `MultipartForm`, and each field becomes a part. No
/// Encoding Object is emitted: the default OpenAPI derives for each part from
/// the field's schema is the one Kynos would write. A request is held to the bounds `T`'s fields declare once its parts are
/// read, and a part that breaks one is a 422 keyed by the field's JSON Pointer,
/// as a [`Json`](super::json::Json) member is; which bounds are enforced is
/// [`constraints`](crate::schema::constraints)' to say.
/// The same wrapper may be returned as a response, described with the same
/// field names and schema in both directions.
/// There is no dynamic-field iterator: a handler that accepts arbitrary part
/// names cannot describe them. For a variable number of uploads, declare one
/// field of type `Vec<FilePart>`, marked `#[serde(default)]`: an empty list is
/// no part at all, which the description then admits.
///
/// The parts `T`'s description requires are the parts the decoder requires:
/// `#[derive(MultipartForm)]` documents the serde attributes that decide
/// which, and the ones it refuses because a part cannot follow them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MultipartForm<T>(pub T);

/// The media type decoded and described.
pub(super) const MEDIA_TYPE: &str = mime_names::MULTIPART_FORM_DATA;

/// RFC 2046's delimiter parameter.
const BOUNDARY: &str = "boundary";

/// The parameters this codec reads: [`BOUNDARY`].
pub(super) const PARAMETERS: &[&str] = &[BOUNDARY];

/// One uploaded file within a [`MultipartForm`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilePart {
    /// The client-supplied file name, if any.
    pub file_name: Option<String>,
    /// The declared media type of this part.
    pub content_type: Option<String>,
    /// The part's bytes. Decoded, they share the request body's buffer, which
    /// a part kept past the request keeps alive.
    pub bytes: bytes::Bytes,
}

/// The empty schema: raw binary sits outside JSON Schema's `type`, and the
/// part's media type is the Encoding Object's to state. See `docs/schema.md`.
impl Schema for FilePart {
    // The empty Schema Object is `kynos_openapi::Schema`'s default, so the
    // `Default::default()` mutant is this body and no test can tell them apart.
    #[cfg_attr(test, mutants::skip)]
    fn schema(_registry: &mut Registry) -> kynos_openapi::Schema {
        kynos_openapi::Schema::Object(Box::default())
    }
}

/// One part of a `multipart/form-data` body, with the field name it carries.
///
/// A part always has a name: RFC 7578 requires a `Content-Disposition` naming
/// its form field, so a part without one is malformed.
///
/// [`FromMultipart`] receives these and
/// [`IntoMultipart`](crate::response::codec::multipart::IntoMultipart)
/// produces them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Part {
    /// The form field this part carries.
    pub name: String,
    /// The client-supplied file name, if any.
    pub file_name: Option<String>,
    /// The declared media type of this part.
    pub content_type: Option<String>,
    /// The part's bytes. Decoded, they share the request body's buffer, which
    /// a part kept past the request keeps alive.
    pub bytes: Bytes,
}

/// Drops the name, which the declaring field already records.
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
/// The role `DeserializeOwned` plays for [`Json`](super::json::Json) and
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
/// Multipart's counterpart to the [`FromStr`](std::str::FromStr) that
/// [`params`](crate::extract::params) decodes a parameter through.
///
/// Implemented for [`FilePart`], [`Bytes`] and every [`ParamValue`], so a
/// closed vocabulary such as an enum deriving `ParamValue` is a form field the
/// way it is a query parameter. `#[derive(MultipartForm)]` reads an `Option<T>`
/// field as an optional part and a `Vec<T>` field as a repeated one, so an
/// implementation only ever answers for a single part.
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
        failures: BTreeMap::from([(Pointer::root().member(name).to_string(), detail.to_owned())]),
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

/// The part's text read with `FromStr`, as a parameter's is. A part that is not
/// UTF-8, or whose text `T` refuses, is a 422 keyed by the part's name, with
/// `FromStr`'s error as the detail.
///
/// Any `T` but `String` parses the octets in place. `String` takes them over
/// instead of copying them through `FromStr`: octets nothing else holds become
/// the string in place, shared ones are copied, and the string is shrunk to its
/// length, so a short field never keeps the rest of the request body alive.
///
/// `'static` because that is what tells `String` apart; a part is owned, so no
/// value built from one borrows anyway.
impl<T: ParamValue + 'static> FromPart for T {
    fn from_part(part: Part) -> Result<Self, BodyRejection> {
        if TypeId::of::<T>() == TypeId::of::<String>() {
            let mut text = Some(owned_text(part)?);
            let text = (&mut text as &mut dyn Any)
                .downcast_mut::<Option<T>>()
                .and_then(Option::take);
            return Ok(text.expect("`T` is `String`"));
        }

        std::str::from_utf8(&part.bytes)
            .map_err(|_| not_utf8(&part.name))?
            .parse()
            .map_err(|error: T::Err| malformed_part(&part.name, &error.to_string()))
    }
}

/// The 422 a part whose octets are not text produces.
fn not_utf8(name: &str) -> BodyRejection {
    malformed_part(name, "the part is not valid UTF-8")
}

/// A part's octets as an owned string, shrunk because taking over the last
/// reference to a buffer takes over all of it.
fn owned_text(part: Part) -> Result<String, BodyRejection> {
    let mut text = String::from_utf8(Vec::from(part.bytes)).map_err(|_| not_utf8(&part.name))?;
    text.shrink_to_fit();
    Ok(text)
}

/// The delimiter the request declares, or the rejection saying why there is
/// none to read the body with.
///
/// Read by the parameter rule [`offers`](super::offers) accepted the media type
/// by, so optional whitespace around each `;` (RFC 9110 section 5.6.6) is
/// skipped here as it is there, and a `;` inside a quoted value ends nothing. An unacceptable media type is the 415; an
/// accepted one without a `boundary`, or with an empty one, is a 400, since no
/// parser can find the parts.
fn boundary(headers: &HeaderMap) -> Result<String, BodyRejection> {
    if !super::offers(headers, MEDIA_TYPE) {
        return Err(super::unsupported_media_type(headers));
    }

    super::content_type(headers)
        .and_then(|(_, parameters)| {
            crate::http::unquoted_split(parameters, b';')
                .filter_map(super::parameter)
                .find(|(name, _)| name.eq_ignore_ascii_case(BOUNDARY))
        })
        .map(|(_, value)| value)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| BodyRejection::Syntax {
            detail: format!("`{MEDIA_TYPE}` declares no `{BOUNDARY}` parameter"),
        })
}

/// A parser failure is a 400, except passing the limit, which is the 413.
fn malformed_body(error: &multer::Error) -> BodyRejection {
    match error {
        multer::Error::StreamSizeExceeded { limit } => BodyRejection::TooLarge { limit: *limit },
        _ => BodyRejection::Syntax {
            detail: error.to_string(),
        },
    }
}

/// A part's octets.
///
/// `multer` yields a part it holds whole as one chunk sharing the body's
/// buffer, which is kept rather than copied; its own `bytes` copies every part
/// into a fresh buffer and boxes it again to share it. A part spread over
/// several chunks is joined.
async fn octets(mut field: multer::Field<'_>) -> Result<Bytes, multer::Error> {
    let Some(first) = field.chunk().await? else {
        return Ok(Bytes::new());
    };
    let Some(second) = field.chunk().await? else {
        return Ok(first);
    };

    let mut joined = Vec::with_capacity(first.len() + second.len());
    joined.extend_from_slice(&first);
    joined.extend_from_slice(&second);
    while let Some(chunk) = field.chunk().await? {
        joined.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(joined))
}

/// `T: Schema` because the bounds a derived field declares are checked once `T`
/// is built, each part under its field's name.
impl<C: Sync, T: FromMultipart + Schema + Send> FromRequest<C> for MultipartForm<T> {
    type Rejection = BodyRejection;

    // `head`, since `parts` names the form's parts below.
    async fn from_request(
        head: &mut Parts,
        body: Body,
        _context: &C,
    ) -> Result<Self, Self::Rejection> {
        let boundary = boundary(&head.headers)?;
        super::refuse_undecoded(&head.headers)?;

        // Every part is held until `T` is built, so the limit bounds the whole
        // stream.
        let limit = super::limit::of(head);
        super::limit::refuse_declared(&head.headers, limit)?;
        let constraints =
            multer::Constraints::new().size_limit(multer::SizeLimit::new().whole_stream(limit));
        let mut fields =
            multer::Multipart::with_constraints(body.into_data_stream(), boundary, constraints);

        let mut parts = Vec::new();
        while let Some(field) = fields.next_field().await.map_err(|e| malformed_body(&e))? {
            // Read here rather than through multer's `name()`, whose scan is
            // not quote-aware and never checks the disposition type.
            let disposition::Disposition { name, file_name } = field
                .headers()
                .get(header::CONTENT_DISPOSITION)
                .ok_or("a part carries no `Content-Disposition`")
                .and_then(|value| {
                    std::str::from_utf8(value.as_bytes())
                        .map_err(|_| "a part's `Content-Disposition` is not UTF-8")
                })
                .map_err(str::to_owned)
                .and_then(disposition::parse)
                .map_err(|detail| BodyRejection::Syntax { detail })?;
            let content_type = field
                .content_type()
                .map(|mime| AsRef::<str>::as_ref(mime).to_owned());
            let bytes = octets(field).await.map_err(|e| malformed_body(&e))?;

            parts.push(Part {
                name,
                file_name,
                content_type,
                bytes,
            });
        }

        super::checked(T::from_parts(parts)?, Pointer::root()).map(Self)
    }
}

impl<T: Schema> Describe for MultipartForm<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        super::describe_body::<Self>(operation);
    }
}

impl<T: Schema> RequestContent for MultipartForm<T> {
    fn media_types() -> Vec<&'static str> {
        vec![MEDIA_TYPE]
    }

    // No Encoding Object: the specification's per-property default derived
    // from the schema is exactly what Kynos would emit.
    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        kynos_openapi::RequestBody::new(
            MEDIA_TYPE,
            kynos_openapi::MediaType::new(registry.resolve::<T>()),
        )
    }
}
