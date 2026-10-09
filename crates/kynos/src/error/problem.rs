//! RFC 9457 problem details: the one error shape Kynos puts on the wire.

use std::{borrow::Cow, collections::BTreeMap};

use bytes::Bytes;
use serde::ser::SerializeMap;
use serde_json::Value;

use kynos_openapi::{
    ComponentName, Map, MediaType, Response, Schema as OpenApiSchema, SchemaObject,
    model::{
        body::mime_names::APPLICATION_PROBLEM_JSON,
        schema::types::{SchemaType, TypeSet},
    },
};

use crate::{
    http::{HeaderValue, StatusCode, body::Body, header},
    response::IntoResponse,
    schema::{Schema, flatten::Flatten, registry::Registry},
};

#[cfg(test)]
mod tests;

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
/// This is [anti-pattern 4] applied to errors. Name an error type instead and let
/// `#[derive(ApiError)]` produce the problem, so the statuses the operation
/// advertises are a `const`.
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
    /// A key naming one of the five registered members (`type`, `title`,
    /// `status`, `detail`, `instance`) never reaches the wire.
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
/// and emits the [`IntoResponse`] and [`Responses`](crate::response::Responses)
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
/// `Responses` do not follow from it. The trait is public so the derive's
/// output can be named and read.
pub trait IntoProblem {
    /// Converts this error into its wire representation.
    fn into_problem(self) -> Problem;

    /// Every status this type can produce.
    ///
    /// The [`Responses`](crate::response::Responses) implementation is derived
    /// from this, so a status returned at runtime but missing here is a bug the
    /// description would hide.
    fn statuses() -> &'static [StatusCode];
}

/// Writes `type` and `status` always (the schema's required members), `title`
/// unless empty, `detail` and `instance` when present, and the extensions
/// flattened alongside, minus any whose key names a registered member.
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

/// Renders the problem as `application/problem+json`. It deliberately does not
/// implement [`Responses`](crate::response::Responses); see the type docs.
impl IntoResponse for Problem {
    fn into_response(self) -> crate::http::Response {
        let status = self.status;
        // Cannot fail in practice; the fallback keeps the response path panic-free.
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

/// The schema every error response references, registered as the named
/// component `Problem`.
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

        // Admit extension members; each problem type decides its own.
        object.additional_properties = Some(Box::new(OpenApiSchema::Bool(true)));
        object.required = Some(vec!["type".to_owned(), "status".to_owned()]);

        OpenApiSchema::Object(Box::new(object))
    }

    fn name() -> Option<ComponentName> {
        ComponentName::new("Problem").ok()
    }
}

/// Flattenable, so a problem type can carry its extension members as fields of
/// its own. Those fields may not reuse `type`, `title`, `status`, `detail` or
/// `instance`.
impl Flatten for Problem {}

/// The description of one response carrying a problem document: the problem
/// media type and the shared component, unnarrowed.
///
/// Returns the response so a caller can chain `with_header` onto it.
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

// --- What a refusal names itself -----------------------------------------

/// The RFC 9457 problem type a refusal names.
///
/// `type` is what a client branches on, so a service distinguishing, say, a
/// burst limit from a spent allowance names each. Implement this on a marker
/// type and select it on the interceptor that owns the refusal.
///
/// ```
/// use kynos::error::problem::ProblemType;
///
/// struct Throttled;
///
/// impl ProblemType for Throttled {
///     const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/rate-limited");
/// }
/// ```
///
/// A type rather than a value because an
/// [`Interceptor`](crate::middleware::Interceptor)'s declaration is read from
/// its types, so the same `const` reaches both the body and the description.
pub trait ProblemType: 'static {
    /// The URI identifying the problem type, or `None` for `about:blank`.
    const TYPE_URI: Option<&'static str>;
}

/// The default: a refusal whose status code is the whole story.
impl ProblemType for () {
    const TYPE_URI: Option<&'static str> = None;
}

/// The problem a refusal puts on the wire, carrying the type `T` names.
///
/// The single constructor for every short circuit. The `title` stays the
/// status code's reason phrase either way (RFC 9457 section 3.1.3).
pub(crate) fn refusal_problem<T: ProblemType>(status: StatusCode) -> Problem {
    let mut problem = Problem::new(status);

    if let Some(uri) = T::TYPE_URI {
        problem.type_uri = uri.into();
    }

    problem
}

/// The response a refusal declares for one status, narrowed to the type `T`
/// names.
///
/// The other half of [`refusal_problem`], reading the same `const`. Narrowed
/// rather than exemplified, since conformance checks validate the schema and
/// never read an `example`; naming no type narrows to `about:blank`.
pub(crate) fn refusal_response<T: ProblemType>(
    registry: &mut Registry,
    status: u16,
    summary: &'static str,
) -> Response {
    let problem = registry.resolve::<Problem>();

    narrowed_response(&problem, status, &[(T::TYPE_URI, Some(summary))])
}

// --- What one status declares --------------------------------------------
//
// A function here rather than macro tokens, so `about:blank` has one spelling;
// the derive reaches it through `__private::problem`.

/// One failure answering with a status: the type URI it publishes, and the
/// summary its declaration gave it.
pub(crate) type Branch = (Option<&'static str>, Option<&'static str>);

/// The response one status declares, narrowed to the types it publishes.
///
/// `problem` is the shared component and `branches` the failures answering with
/// `status`, in declaration order. A branch naming no URI narrows to
/// `about:blank`, since a bare `$ref` would break a `oneOf`'s exactly-one rule.
///
/// # Panics
///
/// If `branches` is empty.
#[must_use]
pub(crate) fn narrowed_response(
    problem: &OpenApiSchema,
    status: u16,
    branches: &[Branch],
) -> Response {
    // From every branch, not the deduplicated ones: prose has no exactly-one rule.
    let description = description(status, branches);
    let distinct = distinct(status, branches);

    let schema = match distinct.as_slice() {
        [] => unreachable!(
            "a status narrows to the failures answering with it: no caller passes an \
             empty branch list"
        ),
        // A lone branch's title would repeat the description.
        [(uri, _)] => narrowed_branch(problem, uri, None),
        several => object(SchemaObject {
            one_of: Some(
                several
                    .iter()
                    .map(|(uri, summary)| narrowed_branch(problem, uri, *summary))
                    .collect(),
            ),
            ..SchemaObject::default()
        }),
    };

    Response::with_content(
        description,
        APPLICATION_PROBLEM_JSON,
        MediaType::new(schema),
    )
}

/// The schema's branches: resolved and deduplicated by URI in declaration
/// order.
///
/// A `oneOf` repeating a `const` would match twice, so a repeated URI keeps
/// only its first summary.
fn distinct(status: u16, branches: &[Branch]) -> Vec<(String, Option<&'static str>)> {
    let mut distinct: Vec<(String, Option<&'static str>)> = Vec::with_capacity(branches.len());

    for (uri, summary) in branches {
        let uri = uri.map_or_else(|| about_blank(status), ToOwned::to_owned);
        if !distinct.iter().any(|(seen, _)| *seen == uri) {
            distinct.push((uri, *summary));
        }
    }

    distinct
}

/// The type URI a failure naming none publishes, read from `Problem` itself.
fn about_blank(status: u16) -> String {
    let status = StatusCode::from_u16(status)
        .expect("`#[derive(ApiError)]` rejects a status outside 400..=599");

    Problem::new(status).type_uri.into_owned()
}

/// The shared component, and the one thing this branch adds to it.
fn narrowed_branch(problem: &OpenApiSchema, uri: &str, summary: Option<&str>) -> OpenApiSchema {
    let mut properties = Map::new();
    properties.insert(
        "type".to_owned(),
        object(SchemaObject {
            ty: Some(TypeSet::One(SchemaType::String)),
            const_value: Some(Value::String(uri.to_owned())),
            ..SchemaObject::default()
        }),
    );

    object(SchemaObject {
        // The problem type's summary (RFC 9457 section 3.1.2), per branch.
        title: summary.map(ToOwned::to_owned),
        all_of: Some(vec![
            problem.clone(),
            object(SchemaObject {
                properties,
                ..SchemaObject::default()
            }),
        ]),
        ..SchemaObject::default()
    })
}

/// The response's description: every summary the status's failures gave, in
/// declaration order and without repeats, falling back to the code's own
/// reason phrase.
fn description(status: u16, branches: &[Branch]) -> String {
    let mut summaries: Vec<&str> = Vec::with_capacity(branches.len());
    for (_, summary) in branches {
        match summary {
            Some(summary) if !summaries.contains(summary) => summaries.push(summary),
            _ => {}
        }
    }

    if summaries.is_empty() {
        return StatusCode::from_u16(status)
            .ok()
            .and_then(|status| status.canonical_reason())
            .unwrap_or("the request failed")
            .to_owned();
    }

    summaries.join("; ")
}

/// A keyword-carrying schema.
fn object(object: SchemaObject) -> OpenApiSchema {
    OpenApiSchema::Object(Box::new(object))
}
