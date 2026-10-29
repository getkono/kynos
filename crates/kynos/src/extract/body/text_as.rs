//! A UTF-8 text body under a declared media type.

use std::marker::PhantomData;

use crate::{
    error::rejection::BodyRejection,
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
    },
    http::{
        Parts,
        body::Body,
        media::{MediaType, WellFormed, is_text, is_utf8_or_unstated},
    },
    router::operation::OperationCx,
    schema::registry::Registry,
};

/// A UTF-8 text request or response body carried under the `text/*` media
/// type `M` names.
///
/// What [`Text`](super::text::Text) is for `text/plain`, this is for any other
/// text format, such as `text/csv`, `text/markdown` or a vendor `text/vnd.*`:
/// the same decoder, the same 400 for bytes that are not UTF-8, and a `string`
/// schema in the description, keyed by `M::MEDIA_TYPE` rather than
/// `text/plain`. Serving bytes without decoding them, such as a pre-rendered
/// file, stays [`Binary`](super::binary::Binary).
///
/// `M` names a `text` type with a subtype, in any ASCII case, and declares no
/// `charset` but `utf-8`: Kynos decodes only UTF-8, so a marker such as
/// `text/csv; charset=iso-8859-1` would mis-decode every body it reads. Either
/// mistake fails the build wherever the body is extracted, written, described
/// or offered:
///
/// ```compile_fail
/// use kynos::{extract::{body::text_as::TextAs, describe::RequestContent}, http::media::Png};
///
/// TextAs::<Png>::media_types();
/// ```
///
/// ```compile_fail
/// use kynos::{extract::body::text_as::TextAs, http::media::MediaType, response::IntoResponse};
///
/// struct Latin1Csv;
/// impl MediaType for Latin1Csv {
///     const MEDIA_TYPE: &'static str = "text/csv; charset=iso-8859-1";
/// }
///
/// TextAs::<Latin1Csv>::new("a,b").into_response();
/// ```
///
/// ```compile_fail
/// use kynos::{
///     extract::body::{text::Text, text_as::TextAs},
///     http::media::Pdf,
///     response::negotiate::representation::Representations,
/// };
///
/// <(Text, TextAs<Pdf>)>::media_types();
/// ```
///
/// A request matches `M` by type and subtype, as for `Binary`: it need not
/// repeat a parameter the marker declares, may not contradict one, and may add
/// `charset=utf-8`. A missing or different content type, or another charset,
/// rejects with 415, and bytes that are not UTF-8 with 400.
///
/// A response carries `M::MEDIA_TYPE` as its `Content-Type`, with
/// `; charset=utf-8` appended when the marker states no charset, as `Text`
/// states its own: RFC 6657 removed the US-ASCII default of `text/*`.
///
/// ```
/// use kynos::{extract::body::text_as::TextAs, http::media::MediaType, response::IntoResponse};
///
/// struct Csv;
/// impl MediaType for Csv {
///     const MEDIA_TYPE: &'static str = "text/csv";
/// }
///
/// let written = TextAs::<Csv>::new("id,name\n1,lamp\n").into_response();
/// assert_eq!(written.headers()["content-type"], "text/csv; charset=utf-8");
/// ```
///
/// Beside another body in a [`OneOf`](super::OneOf), the two must be offered
/// different requests, so two `TextAs` of one type and subtype are refused,
/// whatever parameters tell them apart,
///
/// ```compile_fail
/// use kynos::{
///     extract::{body::{OneOf, text_as::TextAs}, describe::RequestContent},
///     http::media::MediaType,
/// };
///
/// struct Csv;
/// impl MediaType for Csv {
///     const MEDIA_TYPE: &'static str = "text/csv";
/// }
/// struct HeaderedCsv;
/// impl MediaType for HeaderedCsv {
///     const MEDIA_TYPE: &'static str = "TEXT/CSV; header=present";
/// }
///
/// OneOf::<TextAs<Csv>, TextAs<HeaderedCsv>>::media_types();
/// ```
///
/// as is one naming `text/plain` beside a `Text`,
///
/// ```compile_fail
/// use kynos::{
///     extract::{body::{OneOf, text::Text, text_as::TextAs}, describe::RequestContent},
///     http::media::MediaType,
/// };
///
/// struct Plain;
/// impl MediaType for Plain {
///     const MEDIA_TYPE: &'static str = "text/plain; charset=utf-8";
/// }
///
/// OneOf::<Text, TextAs<Plain>>::media_types();
/// ```
///
/// one beside a `Binary` of its own type and subtype,
///
/// ```compile_fail
/// use kynos::{
///     extract::{body::{OneOf, binary::Binary, text_as::TextAs}, describe::RequestContent},
///     http::media::Html,
/// };
///
/// OneOf::<TextAs<Html>, Binary<Html>>::media_types();
/// ```
///
/// and one beside a `JsonAs` whose marker is the same `text/*` type with a
/// `+json` suffix, which both bodies accept, in either order:
///
/// ```compile_fail
/// use kynos::{
///     extract::{body::{OneOf, json_as::JsonAs, text_as::TextAs}, describe::RequestContent},
///     http::media::MediaType,
/// };
///
/// struct Example;
/// impl MediaType for Example {
///     const MEDIA_TYPE: &'static str = "text/vnd.example+json";
/// }
///
/// OneOf::<TextAs<Example>, JsonAs<Example, u64>>::media_types();
/// ```
///
/// ```compile_fail
/// use kynos::{
///     extract::{body::{OneOf, json_as::JsonAs, text_as::TextAs}, describe::RequestContent},
///     http::media::MediaType,
/// };
///
/// struct Example;
/// impl MediaType for Example {
///     const MEDIA_TYPE: &'static str = "text/vnd.example+json";
/// }
///
/// OneOf::<JsonAs<Example, u64>, TextAs<Example>>::media_types();
/// ```
///
/// Distinct types are alternatives, in a request and in a response offer:
///
/// ```
/// use kynos::{
///     extract::{body::{OneOf, text::Text, text_as::TextAs}, describe::RequestContent},
///     http::media::{Html, MediaType},
///     response::negotiate::representation::Representations,
/// };
///
/// struct Csv;
/// impl MediaType for Csv {
///     const MEDIA_TYPE: &'static str = "text/csv";
/// }
///
/// let offered = OneOf::<OneOf<TextAs<Csv>, TextAs<Html>>, Text>::media_types();
/// assert_eq!(offered, ["text/csv", "text/html; charset=utf-8", "text/plain"]);
///
/// let offered = <(Text, TextAs<Csv>, TextAs<Html>)>::media_types();
/// assert_eq!(offered.len(), 3);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextAs<M> {
    /// The decoded text.
    pub text: String,
    media: PhantomData<M>,
}

impl<M> TextAs<M> {
    /// Wraps text with its compile-time media type.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            media: PhantomData,
        }
    }

    /// Takes the text out.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.text
    }
}

impl<M: MediaType> TextAs<M> {
    /// Refuses, in the constant evaluation reading it, a marker that is not
    /// [`WellFormed`], does not name a `text/*` type, or declares a charset
    /// other than UTF-8.
    pub(crate) const TEXT: () = {
        let () = <M as WellFormed>::WELL_FORMED;
        assert!(
            is_text(M::MEDIA_TYPE),
            "a `TextAs` marker does not name a `text/*` type, and its body is decoded as text"
        );
        assert!(
            is_utf8_or_unstated(M::MEDIA_TYPE),
            "a `TextAs` marker declares a charset other than `utf-8`, and its body is decoded \
             as UTF-8"
        );
    };
}

impl<C, M> FromRequest<C> for TextAs<M>
where
    C: Sync,
    M: MediaType + Send,
{
    type Rejection = BodyRejection;

    async fn from_request(
        parts: &mut Parts,
        body: Body,
        _context: &C,
    ) -> Result<Self, Self::Rejection> {
        let () = Self::TEXT;

        let bytes = super::read_body(parts, body, M::MEDIA_TYPE).await?;
        super::text::decode(bytes).map(Self::new)
    }
}

impl<M: MediaType> Describe for TextAs<M> {
    fn describe(operation: &mut OperationCx<'_>) {
        super::describe_body::<Self>(operation);
    }
}

impl<M: MediaType> RequestContent for TextAs<M> {
    fn media_types() -> Vec<&'static str> {
        let () = Self::TEXT;
        vec![M::MEDIA_TYPE]
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        let () = Self::TEXT;
        kynos_openapi::RequestBody::new(
            M::MEDIA_TYPE,
            kynos_openapi::MediaType::new(registry.resolve::<String>()),
        )
    }
}
