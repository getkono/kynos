//! The `multipart/form-data` body codec.

use std::collections::BTreeMap;

use bytes::Bytes;
use http_body_util::BodyExt;
use kynos_openapi::model::body::mime_names;

use crate::{
    error::rejection::BodyRejection,
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
    },
    http::{HeaderMap, Request, header},
    router::operation::OperationCx,
    schema::{Schema, constraints::Pointer, registry::Registry},
};

/// A `multipart/form-data` request body with declared fields.
///
/// `T` derives `Schema`, and each field becomes a part with its own `Encoding`.
/// A request is held to the bounds `T`'s fields declare once its parts are
/// read, and a part that breaks one is a 422 keyed by the field's JSON Pointer,
/// as a [`Json`](super::json::Json) member is; which bounds are enforced is
/// [`constraints`](crate::schema::constraints)' to say.
/// The same wrapper may be returned as a response, preserving the declared
/// field names, per-part media types, and encodings in both directions.
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
    /// The part's bytes.
    pub bytes: bytes::Bytes,
}

/// The empty schema: raw binary sits outside JSON Schema's `type`, and the
/// part's media type is the Encoding Object's to state. See `docs/schema.md`.
impl Schema for FilePart {
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
    /// The part's bytes.
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
/// Implemented for [`FilePart`], `String` and [`Bytes`]. `#[derive(MultipartForm)]`
/// reads an `Option<T>` field as an optional part and a `Vec<T>` field as a
/// repeated one, so an implementation only ever answers for a single part.
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

/// A part that is not UTF-8 is a 422.
impl FromPart for String {
    fn from_part(part: Part) -> Result<Self, BodyRejection> {
        Self::from_utf8(part.bytes.to_vec())
            .map_err(|_| malformed_part(&part.name, "the part is not valid UTF-8"))
    }
}

/// The delimiter the request declares, or the rejection saying why there is
/// none to read the body with.
///
/// Read by the parameter rule [`offers`](super::offers) accepted the media type
/// by, so optional whitespace around each `;` (RFC 9110 section 5.6.6) is
/// skipped here as it is there. An unacceptable media type is the 415; an
/// accepted one without a `boundary`, or with an empty one, is a 400, since no
/// parser can find the parts.
fn boundary(headers: &HeaderMap) -> Result<String, BodyRejection> {
    if !super::offers(headers, MEDIA_TYPE) {
        return Err(super::unsupported_media_type(headers));
    }

    super::content_type(headers)
        .and_then(|(_, parameters)| {
            parameters
                .split(';')
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

/// The disposition RFC 7578 section 4.2 requires of every part.
const FORM_DATA: &str = "form-data";

/// What a part's `Content-Disposition` says about the field it carries.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Disposition {
    /// The `name` parameter.
    pub(crate) name: String,
    /// The `filename` parameter, if any.
    pub(crate) file_name: Option<String>,
}

/// Reads a part's `Content-Disposition` value, or says why it names no field.
///
/// RFC 7578 section 4.2 under RFC 6266 section 4.1's grammar: the type is
/// `form-data`, in any case, followed by `;`-separated parameters whose names
/// are case-insensitive and whose values are a `token` or a `quoted-string`
/// (RFC 9110 sections 5.6.2 and 5.6.4), so a `;` or `name=` inside quotes is
/// part of the value. Whitespace is skipped around `;` and `=`, and an empty
/// parameter between two `;` is skipped. `name` is required; `name` and
/// `filename` may each appear once, since RFC 6266 makes a repeated parameter
/// invalid. Any other parameter is read past and ignored, `filename*`
/// included, which RFC 7578 section 4.2 tells senders not to use. A value is
/// returned as sent, unescaped but not percent-decoded: RFC 7578 section
/// 5.1.3 leaves a percent-encoded name indistinguishable from a literal one.
pub(crate) fn disposition(field: &str) -> Result<Disposition, String> {
    let (kind, mut rest) = token(skip_ows(field));
    if !kind.eq_ignore_ascii_case(FORM_DATA) {
        return Err(format!("a part's disposition is not `{FORM_DATA}`"));
    }

    let malformed = || format!("a part's `Content-Disposition` is malformed: `{field}`");
    let (mut name, mut file_name) = (None, None);
    while !skip_ows(rest).is_empty() {
        rest = skip_ows(skip_ows(rest).strip_prefix(';').ok_or_else(malformed)?);
        if rest.is_empty() || rest.starts_with(';') {
            continue;
        }

        let (parameter, after) = token(rest);
        let after = (skip_ows(after).strip_prefix('=').map(skip_ows))
            .filter(|_| crate::http::is_token(parameter))
            .ok_or_else(malformed)?;
        let value;
        (value, rest) = if after.starts_with('"') {
            quoted_string(after)
        } else {
            let (value, after) = token(after);
            crate::http::is_token(value).then(|| (value.to_owned(), after))
        }
        .ok_or_else(malformed)?;

        let slot = if parameter.eq_ignore_ascii_case("name") {
            &mut name
        } else if parameter.eq_ignore_ascii_case("filename") {
            &mut file_name
        } else {
            continue;
        };
        if slot.replace(value).is_some() {
            return Err(format!(
                "a part's `Content-Disposition` repeats `{}`",
                parameter.to_ascii_lowercase()
            ));
        }
    }

    let name = name.ok_or("a part declares no field name in its `Content-Disposition`")?;
    Ok(Disposition { name, file_name })
}

/// `input` past its leading RFC 9110 `OWS`.
fn skip_ows(input: &str) -> &str {
    input.trim_start_matches([' ', '\t'])
}

/// `input` split where a `token` would have to end; whether the head is one
/// is the caller's to check.
fn token(input: &str) -> (&str, &str) {
    let end = input
        .find([' ', '\t', ';', '=', '"'])
        .unwrap_or(input.len());
    input.split_at(end)
}

/// The leading RFC 9110 section 5.6.4 `quoted-string` of `input`, unescaped,
/// and what follows it; `None` when unterminated or holding a control
/// character. A non-ASCII character is `obs-text`, read as sent.
fn quoted_string(input: &str) -> Option<(String, &str)> {
    // HTAB, SP, VCHAR and obs-text: what `qdtext` and a `quoted-pair` admit.
    let admitted = |c: char| c == '\t' || (c >= ' ' && c != '\u{7f}');
    let mut value = String::new();
    let mut chars = input.strip_prefix('"')?.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '"' => return Some((value, &input[1 + at + 1..])),
            '\\' => value.push(chars.next().map(|(_, c)| c).filter(|&c| admitted(c))?),
            c if admitted(c) => value.push(c),
            _ => return None,
        }
    }
    None
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

/// `T: Schema` because the bounds a derived field declares are checked once `T`
/// is built, each part under its field's name.
impl<C: Sync, T: FromMultipart + Schema + Send> FromRequest<C> for MultipartForm<T> {
    type Rejection = BodyRejection;

    async fn from_request(request: Request, _context: &C) -> Result<Self, Self::Rejection> {
        let boundary = boundary(request.headers())?;
        super::refuse_undecoded(request.headers())?;

        // Every part is held until `T` is built, so the limit bounds the whole
        // stream.
        let limit = super::limit::of(&request);
        super::limit::refuse_declared(request.headers(), limit)?;
        let constraints =
            multer::Constraints::new().size_limit(multer::SizeLimit::new().whole_stream(limit));
        let mut fields = multer::Multipart::with_constraints(
            request.into_body().into_data_stream(),
            boundary,
            constraints,
        );

        let mut parts = Vec::new();
        while let Some(field) = fields.next_field().await.map_err(|e| malformed_body(&e))? {
            // Read here rather than through multer's `name()`, whose scan is
            // not quote-aware and never checks the disposition type.
            let Disposition { name, file_name } = field
                .headers()
                .get(header::CONTENT_DISPOSITION)
                .ok_or("a part carries no `Content-Disposition`")
                .and_then(|value| {
                    std::str::from_utf8(value.as_bytes())
                        .map_err(|_| "a part's `Content-Disposition` is not UTF-8")
                })
                .map_err(str::to_owned)
                .and_then(disposition)
                .map_err(|detail| BodyRejection::Syntax { detail })?;
            let content_type = field.content_type().map(ToString::to_string);
            let bytes = field.bytes().await.map_err(|e| malformed_body(&e))?;

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
        let body = <Self as RequestContent>::request_body(operation.registry());
        operation.set_request_body(body);
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
