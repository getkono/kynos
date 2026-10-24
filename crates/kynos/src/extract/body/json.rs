//! The `application/json` body codec.

use std::collections::BTreeMap;

use kynos_openapi::model::body::mime_names;

use crate::{
    error::rejection::BodyRejection,
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
    },
    http::{Parts, body::Body},
    router::operation::OperationCx,
    schema::{Schema, constraints::Pointer, registry::Registry},
};

/// An `application/json` request or response body.
///
/// Requires the default-on `json` feature. Requests accept
/// `application/json` with no parameters or with `charset=utf-8`; a missing or
/// different content type rejects with 415. Malformed or incomplete JSON
/// rejects with 400, while valid JSON that cannot deserialize into `T` or
/// breaks a bound `T`'s schema declares rejects with 422, keyed by the JSON
/// Pointer of each member that broke one. A top-level array is not a struct
/// `T`, though `serde` would read one from its members. Which bounds are
/// enforced is [`constraints`](crate::schema::constraints)' to say.
///
/// Extracting one therefore requires `T: Schema` as well as
/// `T: DeserializeOwned`; a type extracted outside a described handler
/// derives [`Schema`] for the bounds it is held to.
///
/// ```no_run
/// use kynos::extract::body::json::Json;
///
/// async fn echo(Json(message): Json<String>) -> Json<String> {
///     Json(message)
/// }
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Json<T>(pub T);

/// The media type decoded and described.
pub(super) const MEDIA_TYPE: &str = mime_names::APPLICATION_JSON;

/// `T: Schema` because the bounds a derived field declares are checked once `T`
/// is deserialized.
impl<C: Sync, T: serde::de::DeserializeOwned + Schema + Send> FromRequest<C> for Json<T> {
    type Rejection = BodyRejection;

    async fn from_request(
        parts: &mut Parts,
        body: Body,
        _context: &C,
    ) -> Result<Self, Self::Rejection> {
        let bytes = super::read_body(parts, body, MEDIA_TYPE).await?;
        let value = from_slice(&bytes).map_err(rejection)?;
        super::checked(value, Pointer::root()).map(Self)
    }
}

/// Reads one JSON document as [`serde_json::from_slice`] does, except that a
/// struct at the top level is read only from an object.
///
/// `serde`'s derive also reads a struct from an array of its members, which
/// the `type: object` its schema declares refuses; where every member
/// defaults, it reads `[]` as `{}`. The refusal comes before any member is
/// read, and reaches through `Option` and newtypes; a struct nested in the
/// document is `serde`'s to read. Bytes that are not one JSON document keep
/// their syntax error, so the 400/422 split holds.
pub(super) fn from_slice<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, serde_json::Error> {
    let array = bytes
        .iter()
        .find(|byte| !matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
        == Some(&b'[');
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = T::deserialize(TopLevel {
        inner: &mut deserializer,
        bytes,
        array,
    })?;
    deserializer.end()?;
    Ok(value)
}

/// The top level of a document, refusing to read a struct from an array.
struct TopLevel<'a, 'de> {
    /// What every request is forwarded to.
    inner: &'a mut serde_json::Deserializer<serde_json::de::SliceRead<'de>>,

    /// The whole document, checked for well-formedness before an array is
    /// refused.
    bytes: &'de [u8],

    /// Whether the document's first token opens an array.
    array: bool,
}

/// Forwards each named `deserialize_*` method, with its extra arguments, to
/// the inner deserializer.
macro_rules! forward {
    ($($method:ident($($argument:ident: $kind:ty),*);)*) => {$(
        fn $method<V: serde::de::Visitor<'de>>(
            self,
            $($argument: $kind,)*
            visitor: V,
        ) -> Result<V::Value, Self::Error> {
            self.inner.$method($($argument,)* visitor)
        }
    )*};
}

impl<'de> serde::Deserializer<'de> for TopLevel<'_, 'de> {
    type Error = serde_json::Error;

    forward! {
        deserialize_any();
        deserialize_bool();
        deserialize_i8();
        deserialize_i16();
        deserialize_i32();
        deserialize_i64();
        deserialize_i128();
        deserialize_u8();
        deserialize_u16();
        deserialize_u32();
        deserialize_u64();
        deserialize_u128();
        deserialize_f32();
        deserialize_f64();
        deserialize_char();
        deserialize_str();
        deserialize_string();
        deserialize_bytes();
        deserialize_byte_buf();
        deserialize_unit();
        deserialize_unit_struct(name: &'static str);
        deserialize_seq();
        deserialize_tuple(len: usize);
        deserialize_tuple_struct(name: &'static str, len: usize);
        deserialize_map();
        deserialize_enum(name: &'static str, variants: &'static [&'static str]);
        deserialize_identifier();
        deserialize_ignored_any();
    }

    fn deserialize_struct<V: serde::de::Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if self.array {
            // A syntax error wins: the refusal is only for well-formed JSON.
            serde_json::from_slice::<serde::de::IgnoredAny>(self.bytes)?;
            Err(serde::de::Error::invalid_type(
                serde::de::Unexpected::Seq,
                &"a JSON object",
            ))
        } else {
            self.inner.deserialize_struct(name, fields, visitor)
        }
    }

    /// An array is not `null`, so it is `Some` of what the array holds, as
    /// `serde_json` would read it, but still at the top level.
    fn deserialize_option<V: serde::de::Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if self.array {
            visitor.visit_some(self)
        } else {
            self.inner.deserialize_option(visitor)
        }
    }

    /// A newtype is its member, as `serde_json` reads it, but still at the top
    /// level; `serde_json`'s own private tokens, such as `RawValue`'s, are its
    /// to read.
    fn deserialize_newtype_struct<V: serde::de::Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if name.starts_with("$serde_json::") {
            self.inner.deserialize_newtype_struct(name, visitor)
        } else {
            visitor.visit_newtype_struct(self)
        }
    }
}

/// Malformed JSON is a 400; well-formed JSON that does not fit `T` is a 422.
///
/// Keyed at the root pointer: serde reports a line and column, not a location.
// By value to fit `map_err`.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn rejection(error: serde_json::Error) -> BodyRejection {
    if is_schema_failure(&error) {
        BodyRejection::Schema {
            failures: BTreeMap::from([(String::new(), error.to_string())]),
        }
    } else {
        BodyRejection::Syntax {
            detail: error.to_string(),
        }
    }
}

/// Where the 400/422 line falls for every JSON codec: serde's `Data` category
/// is a value that does not fit the type; the rest are bytes that are not JSON.
pub(super) fn is_schema_failure(error: &serde_json::Error) -> bool {
    match error.classify() {
        serde_json::error::Category::Data => true,
        serde_json::error::Category::Io
        | serde_json::error::Category::Syntax
        | serde_json::error::Category::Eof => false,
    }
}

impl<T: Schema> Describe for Json<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let body = <Self as RequestContent>::request_body(operation.registry());
        operation.set_request_body(body);
    }
}

impl<T: Schema> RequestContent for Json<T> {
    fn media_types() -> Vec<&'static str> {
        vec![MEDIA_TYPE]
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        kynos_openapi::RequestBody::json(registry.resolve::<T>())
    }
}
