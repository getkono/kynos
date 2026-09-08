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
    schema::{Schema, registry::Registry},
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

// --- What a refusal names itself -----------------------------------------

/// The RFC 9457 problem type a refusal names.
///
/// `type` is what a client branches on, and `about:blank` says "the status code
/// is the whole story" — true of a generic refusal and false of a service that
/// distinguishes a burst limit from a spent monthly allowance, or a 503 from a
/// concurrency cap from a 503 from a shed queue. Implement this on a marker
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
/// # Why a type rather than a value
///
/// What an interceptor declares is read from its associated types and never
/// from an instance — see [`Interceptor`](crate::middleware::Interceptor),
/// which has no `contribution` method for exactly this reason. A URI supplied
/// at run time could therefore reach the wire and nothing else, leaving the
/// document saying `about:blank` about a response that says otherwise. Stated
/// as a type, the same `const` reaches both halves: one function builds the
/// body and one narrows the declaration, both reading this constant.
///
/// The const has no default. It is the one thing this trait carries, and a
/// marker that left it unwritten would compile, ship `about:blank`, declare
/// `about:blank`, and produce no diagnostic saying the feature had silently
/// done nothing.
///
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
/// One function for every short circuit, because the document is a claim about
/// what the wire carries and two constructions of "the same" problem are how
/// the two came to disagree. The `title` stays the status code's reason phrase
/// whether or not a type was named: RFC 9457 section 3.1.3 makes it a summary
/// of the problem *type*, and a refusal's reason phrase summarises every
/// refusal of that kind there is.
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
/// rather than exemplified: [`assert_conformance`] validates a body against the
/// declared `schema` and never reads an `example`, so a refusal whose body
/// disagreed with an exemplified declaration passed.
///
/// A refusal naming no type narrows to `about:blank`, which is what
/// [`Problem::new`] sets and what [`refusal_problem`] leaves alone — the same
/// rule [`rejection`](crate::error::rejection) follows, and for the reason
/// [`narrowed_response`] gives: a bare `$ref` admits every problem document the
/// service can produce, so a status an extractor also claims would lose its own
/// narrowing to this one.
///
/// [`assert_conformance`]: crate::test::TestClient::assert_conformance
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
// Emitted code cannot spell `about:blank`: the URI a problem carrying no
// semantics of its own uses belongs to `Problem` above, and a second spelling
// in `kynos-macros` would be a constant nothing holds to the first. So the
// shape a status's response takes is a function here rather than tokens there,
// and `__private::problem` is the name the expansion reaches it by.

/// One failure answering with a status: the type URI it publishes, and the
/// summary its declaration gave it.
pub(crate) type Branch = (Option<&'static str>, Option<&'static str>);

/// The response one status declares, narrowed to the types it publishes.
///
/// `problem` is the shared component every branch refers to, and `branches` are
/// the failures answering with `status`, in declaration order. The result
/// narrows that component to the type URIs those failures can publish, so a
/// consumer reading the description learns which `type` a body may carry rather
/// than only that it is a problem detail.
///
/// A branch naming no URI narrows to `about:blank`, which is what
/// [`Problem::new`] sets and what the serializer writes. The alternative — a
/// bare `$ref` — would match every problem document and cost a `oneOf` its
/// exactly-one rule.
///
/// `branches` is never empty: a status no failure answers with is not a
/// narrowing of anything, and passing one panics.
///
/// Two callers, and they narrow for the same reason. `#[derive(ApiError)]`
/// passes the failures a status is declared for, through
/// [`__private::problem::response`](crate::__private::problem::response);
/// [`rejection`](crate::error::rejection) passes one branch naming no URI,
/// because every rejection but one publishes `about:blank` and a description
/// that said only "a problem document" would lose to an extractor what the
/// derive just gained.
#[must_use]
pub(crate) fn narrowed_response(
    problem: &OpenApiSchema,
    status: u16,
    branches: &[Branch],
) -> Response {
    // Composed from every branch, not from what survives the URI dedup: the
    // dedup exists to keep a `oneOf` sound, and prose has no such rule.
    let description = description(status, branches);
    let distinct = distinct(status, branches);

    let schema = match distinct.as_slice() {
        // Both callers build this list from failures that named the status, so
        // it is never empty. A future caller that passes nothing is told so,
        // rather than handed the unnarrowed component back and left to believe
        // it narrowed something.
        [] => unreachable!(
            "a status narrows to the failures answering with it: no caller passes an \
             empty branch list"
        ),
        // One branch, so a `title` would repeat what the description already
        // says about the only type this status publishes.
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
/// Two failures may publish one type — the same 404 raised from two call sites
/// — and a `oneOf` repeating a `const` would be satisfied by two branches at
/// once. Where that happens the first summary is the one the surviving branch
/// carries, and [`response`] titles the branch with it only when two or more
/// survive; a single branch is written without a `title`. Either way the
/// description is composed before this and keeps both.
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
        // The summary of the problem *type*, which is what RFC 9457 section
        // 3.1.2 makes `title`. Carried per branch because a `oneOf` is where
        // several of them meet.
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
///
/// A join rather than the first summary, because a status several failures
/// share has several things to say and a response carries one description.
/// Composed from the branches as declared: two failures publishing one URI are
/// one schema branch but remain two failures, and a reader of the description
/// is owed the name of each. Only an identical summary is dropped, since
/// repeating a sentence tells no one anything.
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
