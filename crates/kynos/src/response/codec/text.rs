//! Writing `text/plain`, or the `text/*` type a `TextAs` names, as a response.

use kynos_openapi::model::body::mime_names;

use crate::{
    extract::body::{text::Text, text_as::TextAs},
    http::{
        HeaderValue, Response,
        body::Body,
        media::{MediaType, charset},
    },
    response::{
        IntoResponse, Responses,
        codec::{ok, with_content_type},
    },
    schema::registry::Registry,
};

/// What a `Content-Type` naming no charset has appended.
///
/// RFC 6657 removed the US-ASCII default of `text/*`, and a Rust `String` is
/// UTF-8, so the charset is stated rather than left to the recipient.
const UTF_8: &str = "; charset=utf-8";

/// The room a marker and [`UTF_8`] are built into at compile time.
///
/// A marker past it is stated at run time instead, at the cost of one
/// allocation per response. RFC 6838 allows a type and a subtype of 127
/// characters each, but a registered `text/*` type is a fraction of this.
const CAPACITY: usize = 128;

/// `media_type` with [`UTF_8`] appended, and the length used; a length of zero
/// when the two do not fit in [`CAPACITY`].
const fn with_charset(media_type: &str) -> ([u8; CAPACITY], usize) {
    let mut built = [0; CAPACITY];
    let (media_type, suffix) = (media_type.as_bytes(), UTF_8.as_bytes());
    let len = media_type.len() + suffix.len();
    if len > CAPACITY {
        return (built, 0);
    }

    let at = crate::http::copy_into(&mut built, 0, media_type);
    crate::http::copy_into(&mut built, at, suffix);
    (built, len)
}

/// The one 200 a text body describes: a `string` under `media_type`.
fn text_responses(registry: &mut Registry, media_type: &'static str) -> kynos_openapi::Responses {
    ok(
        media_type,
        kynos_openapi::MediaType::new(registry.resolve::<String>()),
    )
}

/// `text` as a body under `content_type`.
fn text_response(text: String, content_type: HeaderValue) -> Response {
    with_content_type(Body::from_bytes(bytes::Bytes::from(text)), content_type)
}

impl IntoResponse for Text {
    fn into_response(self) -> Response {
        text_response(
            self.0,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        )
    }
}

impl Responses for Text {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        text_responses(registry, mime_names::TEXT_PLAIN)
    }
}

impl<M: MediaType> TextAs<M> {
    /// The marker with [`UTF_8`] appended, built once per marker.
    const WITH_CHARSET: ([u8; CAPACITY], usize) = with_charset(M::MEDIA_TYPE);

    /// The built bytes, borrowed for `'static` so a response can share them.
    const BUILT: &'static [u8; CAPACITY] = &Self::WITH_CHARSET.0;

    /// The `Content-Type` a response states: the marker verbatim when it
    /// declares a charset, else with [`UTF_8`] appended; `None` when that does
    /// not fit in [`CAPACITY`].
    const CONTENT_TYPE: Option<&'static str> = if charset(M::MEDIA_TYPE).is_some() {
        Some(M::MEDIA_TYPE)
    } else if Self::WITH_CHARSET.1 == 0 {
        None
    } else {
        match core::str::from_utf8(Self::BUILT.split_at(Self::WITH_CHARSET.1).0) {
            Ok(built) => Some(built),
            Err(_) => panic!("a media type and an ASCII suffix are UTF-8"),
        }
    };
}

impl<M: MediaType> IntoResponse for TextAs<M> {
    fn into_response(self) -> Response {
        let () = Self::TEXT;
        let content_type = match Self::CONTENT_TYPE {
            Some(content_type) => HeaderValue::from_static(content_type),
            None => HeaderValue::try_from(format!("{}{UTF_8}", M::MEDIA_TYPE))
                // Not a field value: refused as `from_static` refuses `Binary`'s.
                .unwrap_or_else(|_| HeaderValue::from_static(M::MEDIA_TYPE)),
        };
        text_response(self.text, content_type)
    }
}

impl<M: MediaType> Responses for TextAs<M> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let () = Self::TEXT;
        text_responses(registry, M::MEDIA_TYPE)
    }
}
