//! RFC 9457 problem details: the one error shape Kynos puts on the wire.

use std::{borrow::Cow, collections::BTreeMap};

use bytes::Bytes;
use serde::ser::SerializeMap;
use serde_json::Value;

use kynos_openapi::{
    ComponentName, Schema as OpenApiSchema, SchemaObject,
    model::{
        body::mime_names::APPLICATION_PROBLEM_JSON,
        schema::types::{SchemaType, TypeSet},
    },
};

use crate::{
    http::{HeaderValue, StatusCode, body::Body, header},
    response::IntoResponse,
    schema::{Schema, registry::Registry},
};

/// The type URI of a problem carrying no semantics beyond its status code.
///
/// RFC 9457 registers it as the value assumed when `type` is absent; Kynos
/// writes it out, because the schema declares `type` as required.
const ABOUT_BLANK: &str = "about:blank";

/// The members RFC 9457 registers, which an extension may not shadow.
const RESERVED: [&str; 5] = ["type", "title", "status", "detail", "instance"];

/// An RFC 9457 problem detail.
///
/// The five registered members are typed; anything else goes in
/// [`extensions`](Problem::extensions), which is how an error carries the
/// specifics a client needs to act on it — which field failed, which quota was
/// exceeded, when to retry.
///
/// # This is a representation, not a return type
///
/// `Problem` carries its status in a field, so a handler returning one would
/// choose that status at run time and no description could say which. It
/// therefore does not implement [`Responses`](crate::response::Responses), and
/// `Result<T, Problem>` does not compile:
///
/// ```compile_fail
/// # use kynos::{Problem, response::status::NoContent};
/// fn returns<T: kynos::response::IntoResponse + kynos::response::Responses>() {}
/// returns::<Result<NoContent, Problem>>();
/// ```
///
/// This is [anti-pattern 4] applied to errors, and the same reasoning that
/// keeps `IntoResponse` off `StatusCode`. Name an error type instead and let
/// `#[derive(ApiError)]` produce the problem, so the statuses the operation
/// advertises are a `const` rather than whatever the handler happened to build.
///
/// [anti-pattern 4]: https://github.com/getkono/kynos#anti-patterns
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Problem {
    /// A URI identifying the problem *type*.
    ///
    /// Defaults to `about:blank`, which means "the status code is the whole
    /// story". Anything a client should branch on deserves a real URI.
    pub type_uri: Cow<'static, str>,

    /// A short, human-readable summary of the problem type.
    ///
    /// Should not change from occurrence to occurrence; put the specifics in
    /// [`detail`](Problem::detail).
    pub title: Cow<'static, str>,

    /// The HTTP status code.
    pub status: StatusCode,

    /// An explanation specific to this occurrence.
    pub detail: Option<String>,

    /// A URI identifying this specific occurrence.
    pub instance: Option<String>,

    /// Additional members, serialized alongside the registered ones.
    pub extensions: BTreeMap<String, Value>,
}

impl Problem {
    /// Creates a problem with `about:blank` as its type.
    ///
    /// The title is the status code's reason phrase, which is what RFC 9457
    /// asks for when the type carries no semantics of its own.
    #[must_use]
    pub fn new(status: StatusCode) -> Self {
        let title = status.canonical_reason().map_or_else(
            || Cow::Owned(status.as_u16().to_string()),
            Cow::Borrowed::<str>,
        );

        Self::of_type(status, ABOUT_BLANK, title)
    }

    /// Creates a problem with an identifying type URI and title.
    #[must_use]
    pub fn of_type(
        status: StatusCode,
        type_uri: impl Into<Cow<'static, str>>,
        title: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            type_uri: type_uri.into(),
            title: title.into(),
            status,
            detail: None,
            instance: None,
            extensions: BTreeMap::new(),
        }
    }

    /// Sets the occurrence-specific explanation.
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Sets the URI identifying this occurrence.
    #[must_use]
    pub fn with_instance(mut self, instance: impl Into<String>) -> Self {
        self.instance = Some(instance.into());
        self
    }

    /// Attaches an additional member.
    ///
    /// A key naming one of the five registered members never reaches the wire:
    /// an extension that shadowed `type`, `title`, `status`, `detail` or
    /// `instance` would put two entries under one name.
    #[must_use]
    pub fn with_extension(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.extensions.insert(key.into(), value.into());
        self
    }
}

/// A type that becomes an error response.
///
/// **Derive it with `#[derive(ApiError)]`, which is the only supported way to
/// implement it.** The derive maps each variant to a status and a problem type,
/// and — this is the part that matters — emits the
/// [`IntoResponse`] and [`Responses`](crate::response::Responses)
/// implementations at the same time, so the statuses an error can produce and
/// the statuses the description advertises cannot disagree.
///
/// ```no_run
/// use kynos::ApiError;
/// # #[derive(Debug)] struct UserId(u64);
///
/// #[derive(Debug, thiserror::Error, ApiError)]
/// #[problem(base = "https://errors.example.com/")]
/// enum StoreError {
///     #[error("no user with id {0:?}")]
///     #[problem(status = 404, title = "User not found")]
///     NotFound(UserId),
///
///     #[error("that email is already registered")]
///     #[problem(status = 409)]
///     EmailTaken,
/// }
///
/// // The pair the handler bound needs, both emitted from the declaration above.
/// fn returns<T: kynos::response::IntoResponse + kynos::response::Responses>() {}
/// returns::<Result<kynos::response::status::NoContent, StoreError>>();
/// ```
///
/// Implementing this by hand compiles and is not useful: `IntoResponse` and
/// `Responses` do not follow from it, and a blanket implementation over every
/// `IntoProblem` would overlap the concrete ones for `Json<T>`, `Created<T>`
/// and the rest, which Rust rejects rather than resolving. The trait stays
/// public so the derive's output can be named and read, not so it can be
/// reimplemented.
pub trait IntoProblem {
    /// Converts this error into its wire representation.
    fn into_problem(self) -> Problem;

    /// Every status this type can produce.
    ///
    /// The [`Responses`](crate::response::Responses) implementation is derived
    /// from this, so a status returned at runtime but missing here is a bug the
    /// description would hide. The derive computes it from the `status` given
    /// on each variant, which is why the two cannot drift.
    fn statuses() -> &'static [StatusCode];
}

/// Serialized by hand rather than derived: [`StatusCode`] is not
/// [`serde::Serialize`], `type_uri` is written as RFC 9457's `type`, and the
/// extension members are flattened alongside the registered ones rather than
/// nested under a field of their own.
///
/// `type` and `status` are always written, which is what the schema declares as
/// required. `title` is omitted when empty, and `detail` and `instance` when
/// absent. An extension whose key names a registered member is dropped, since
/// one name cannot hold two values.
impl serde::Serialize for Problem {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let extensions = || {
            self.extensions
                .iter()
                .filter(|(key, _)| !RESERVED.contains(&key.as_str()))
        };

        let len = 2
            + usize::from(!self.title.is_empty())
            + usize::from(self.detail.is_some())
            + usize::from(self.instance.is_some())
            + extensions().count();

        let mut map = serializer.serialize_map(Some(len))?;

        map.serialize_entry("type", &self.type_uri)?;
        if !self.title.is_empty() {
            map.serialize_entry("title", &self.title)?;
        }
        map.serialize_entry("status", &self.status.as_u16())?;
        if let Some(detail) = &self.detail {
            map.serialize_entry("detail", detail)?;
        }
        if let Some(instance) = &self.instance {
            map.serialize_entry("instance", instance)?;
        }
        for (key, value) in extensions() {
            map.serialize_entry(key, value)?;
        }

        map.end()
    }
}

/// A problem can be *written*, which is how every error reaches the wire: an
/// `ApiError` converts itself with [`IntoProblem`] and the result is rendered
/// here.
///
/// What it deliberately cannot do is [`Responses`](crate::response::Responses).
/// A handler's return type needs both halves, so the missing one is what stops
/// `Result<T, Problem>` from compiling — see the type documentation for why
/// that matters.
impl IntoResponse for Problem {
    fn into_response(self) -> crate::http::Response {
        let status = self.status;
        // A problem holds strings, a status and JSON values, none of which can
        // fail to serialize. The fallback is there so that a response path
        // never panics: a document naming the status is still a problem
        // document, and the status line stays the one the problem chose.
        let body = serde_json::to_vec(&self).unwrap_or_else(|_| {
            format!(r#"{{"type":"{ABOUT_BLANK}","status":{}}}"#, status.as_u16()).into_bytes()
        });

        let mut response = crate::http::Response::new(Body::from_bytes(Bytes::from(body)));
        *response.status_mut() = status;
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(APPLICATION_PROBLEM_JSON),
        );

        response
    }
}

/// The schema every error response references.
///
/// Registered as a named component rather than inlined, because a document
/// where each of a hundred operations repeats the same five-property object is
/// one no reader will check.
impl Schema for Problem {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        let string = registry.resolve::<String>();
        let integer = registry.resolve::<u16>();

        let mut object = SchemaObject {
            ty: Some(TypeSet::One(SchemaType::Object)),
            ..SchemaObject::default()
        };

        object.title = Some("Problem Details".to_owned());
        object.description = Some("An RFC 9457 problem detail.".to_owned());
        object.properties = [
            ("type".to_owned(), string.clone()),
            ("title".to_owned(), string.clone()),
            ("status".to_owned(), integer),
            ("detail".to_owned(), string.clone()),
            ("instance".to_owned(), string),
        ]
        .into_iter()
        .collect();

        // Extension members are the point of the format, so the schema has to
        // admit them. Constraining them further would be a lie: what a given
        // problem type carries is decided by that type, not by this schema.
        object.additional_properties = Some(Box::new(OpenApiSchema::Bool(true)));
        object.required = Some(vec!["type".to_owned(), "status".to_owned()]);

        OpenApiSchema::Object(Box::new(object))
    }

    fn name() -> Option<ComponentName> {
        ComponentName::new("Problem").ok()
    }
}

/// The description of one response carrying a problem document.
///
/// Every error Kynos puts a *body* on the wire for is an RFC 9457 problem
/// detail, so every description of one names the same media type and the same
/// component. One writer, because eight interceptor short circuits each
/// spelling it by hand is how eight of them came to spell it as nothing at all.
///
/// The qualification is [`FallbackPolicy::Empty`], under which a 404 or a 405
/// answers with the status and no body at all. Such a response is described by
/// declaring no content rather than by this function, and it is the one error
/// Kynos emits that no problem document covers.
///
/// [`FallbackPolicy::Empty`]: crate::router::policy::FallbackPolicy::Empty
///
/// Returns the response rather than a `Responses`, so a caller that also owes a
/// `Retry-After` or an `Accept-Encoding` chains `with_header` onto it.
pub(crate) fn problem_response(
    registry: &mut Registry,
    description: impl Into<String>,
) -> kynos_openapi::Response {
    kynos_openapi::Response::with_content(
        description,
        APPLICATION_PROBLEM_JSON,
        kynos_openapi::MediaType::new(registry.resolve::<Problem>()),
    )
}
