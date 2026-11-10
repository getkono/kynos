//! Extension members an application adds to the problems Kynos renders
//! itself.
//!
//! A rejection, a guard's refusal and a router fallback are Kynos's types,
//! with shapes the application cannot reach. [`Router::problem_members`]
//! names one type, [`ProblemMembers`], that both produces the members each of
//! those problems carries and declares them on every response describing one,
//! so the description cannot drift from the wire (RFC 9457 section 3.2).
//!
//! ```
//! # #[cfg(feature = "macros")]
//! # {
//! use kynos::{
//!     Router, Schema,
//!     error::members::{ProblemCx, ProblemMembers, ProblemOrigin},
//! };
//!
//! /// A stable code a client localizes offline.
//! #[derive(Schema, serde::Serialize)]
//! struct Coded {
//!     code: String,
//! }
//!
//! impl ProblemMembers for Coded {
//!     fn for_problem(cx: &ProblemCx<'_>) -> Self {
//!         let code = match cx.origin() {
//!             ProblemOrigin::Authentication => "auth",
//!             ProblemOrigin::Routing => "route",
//!             _ => "request",
//!         };
//!         Self { code: format!("{code}-{}", cx.status().as_u16()) }
//!     }
//! }
//!
//! let router = Router::<()>::new().problem_members::<Coded>();
//! # let _ = router;
//! # }
//! ```
//!
//! [`Router::problem_members`]: crate::router::Router::problem_members

use std::collections::BTreeMap;

use kynos_openapi::{
    Schema as OpenApiSchema, SchemaObject,
    model::schema::types::{SchemaType, TypeSet},
};
use serde_json::Value;

use crate::{
    error::problem::{Problem, RESERVED},
    http::{Extensions, HeaderMap, Method, Parts, Request, Response, StatusCode, Uri},
    response::IntoResponse,
    router::error::RouterError,
    schema::{Schema, registry::Registry},
};

#[cfg(test)]
mod tests;

/// The member every schema failure sends, which an application's members may
/// not reuse.
const ERRORS: &str = "errors";

/// Which part of Kynos refused the request a problem answers.
///
/// What a problem's status cannot say alone: a 400 is a path parameter, a
/// query parameter, a header or a body that would not decode.
///
/// No variant shares a name with a type Kynos exports: rustc prints a type by
/// its full path wherever a second item shares its name, so a variant named
/// `Body` or `Auth` would lengthen every diagnostic naming those types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ProblemOrigin {
    /// A path parameter: [`PathRejection`](crate::error::rejection::PathRejection).
    PathParameter,
    /// A query parameter or query string:
    /// [`QueryRejection`](crate::error::rejection::QueryRejection).
    QueryParameter,
    /// A request header: [`HeaderRejection`](crate::error::rejection::HeaderRejection).
    HeaderField,
    /// A cookie: `CookieRejection`, behind the `cookie` feature.
    CookieValue,
    /// The request's content, including a `QUERY` naming no media type:
    /// [`BodyRejection`](crate::error::rejection::BodyRejection).
    Content,
    /// The request's `Accept`:
    /// [`NegotiationRejection`](crate::error::rejection::NegotiationRejection).
    Negotiation,
    /// The connection the request arrived on:
    /// [`ConnectInfoRejection`](crate::error::rejection::ConnectInfoRejection).
    Transport,
    /// A credential: [`AuthRejection`](crate::error::rejection::AuthRejection)
    /// and [`ScopedRejection`](crate::error::rejection::ScopedRejection).
    Authentication,
    /// The router, for a request no operation answers: the 404, 405 and 501
    /// under [`FallbackPolicy::Problem`](crate::router::policy::FallbackPolicy::Problem),
    /// and a CORS preflight's answer to an `OPTIONS` that is not one.
    Routing,
    /// The request's `Range`, which no part of the file an asset endpoint
    /// serves satisfies:
    /// [`RangeRejection`](crate::error::rejection::RangeRejection).
    ByteRange,
}

/// What the router knows when it renders a problem it owns.
///
/// Read by [`ProblemMembers::for_problem`]. The request is the head the
/// operation saw, so an extension an interceptor inserted, such as a request
/// ID, is there to read.
#[derive(Debug)]
pub struct ProblemCx<'a> {
    problem: &'a Problem,
    origin: ProblemOrigin,
    head: Head<'a>,
}

impl<'a> ProblemCx<'a> {
    /// The problem as Kynos built it, before any member is added.
    #[must_use]
    pub fn problem(&self) -> &'a Problem {
        self.problem
    }

    /// The problem's status.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.problem.status
    }

    /// Which part of Kynos refused the request.
    #[must_use]
    pub fn origin(&self) -> ProblemOrigin {
        self.origin
    }

    /// The request's method.
    #[must_use]
    pub fn method(&self) -> &'a Method {
        self.head.method
    }

    /// The request's target.
    #[must_use]
    pub fn uri(&self) -> &'a Uri {
        self.head.uri
    }

    /// The request's header fields.
    #[must_use]
    pub fn headers(&self) -> &'a HeaderMap {
        self.head.headers
    }

    /// The request's extensions, as the interceptors outside the refusal left
    /// them.
    #[must_use]
    pub fn extensions(&self) -> &'a Extensions {
        self.head.extensions
    }
}

/// Extension members added to every problem Kynos renders itself, installed
/// with [`Router::problem_members`](crate::router::Router::problem_members).
///
/// Sent through `Serialize` and declared by `Schema`, as a
/// `#[problem(extension)]` field is, so a member sent is a member declared. The
/// schema must be an object's, naming each member as a property: a struct
/// deriving both, with no `#[serde(flatten)]`; `#[serde(deny_unknown_fields)]`
/// may close it, since only its properties are read. A member may not be named like
/// one RFC 9457 registers (`type`, `title`, `status`, `detail`, `instance`) or
/// like `errors`, which a schema failure sends; the router refuses both while
/// it is described.
///
/// A value whose `Serialize` fails, or does not serialize as an object, answers
/// 500 in place of the problem, as an `ApiError` member does.
pub trait ProblemMembers: Schema + serde::Serialize {
    /// The members the problem `cx` describes carries.
    fn for_problem(cx: &ProblemCx<'_>) -> Self;
}

/// The members one problem carries, produced and serialized.
pub(crate) type Produce = fn(&ProblemCx<'_>) -> Result<serde_json::Map<String, Value>, Unsent>;

/// Members that could not be sent: a failed or non-object serialization.
#[derive(Debug)]
pub(crate) struct Unsent;

/// One [`ProblemMembers`] type, erased: how it produces its members and how it
/// declares them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hook {
    pub(crate) produce: Produce,
    describe: fn(&mut Registry) -> OpenApiSchema,
    name: &'static str,
}

impl Hook {
    /// `M`, erased.
    pub(crate) fn of<M: ProblemMembers>() -> Self {
        Self {
            produce: produce::<M>,
            describe: M::schema,
            name: std::any::type_name::<M>(),
        }
    }

    /// Declares the members of the type `hook` erases, where one is installed,
    /// on `registry` for every problem Kynos renders to read; or the violation
    /// refusing them.
    pub(crate) fn declare(hook: Option<Self>, registry: &mut Registry) -> Option<RouterError> {
        match hook?.declared(registry) {
            Ok(members) => {
                registry.declare_problem_members(members);
                None
            }
            Err(refused) => Some(refused),
        }
    }

    /// Each member the type declares: its name, its schema, and whether every
    /// problem sends it.
    ///
    /// Refuses a schema that is not an object's naming its members, and a
    /// member named like one a problem already sends.
    pub(crate) fn declared(
        &self,
        registry: &mut Registry,
    ) -> Result<Vec<(String, OpenApiSchema, bool)>, RouterError> {
        let members = self.name;
        let OpenApiSchema::Object(object) = (self.describe)(registry) else {
            return Err(RouterError::ProblemMembersNotObject { members });
        };
        if !describes_members(&object) {
            return Err(RouterError::ProblemMembersNotObject { members });
        }

        let required = object.required.unwrap_or_default();
        object
            .properties
            .into_iter()
            .map(|(name, schema)| {
                if RESERVED.contains(&name.as_str()) || name == ERRORS {
                    return Err(RouterError::ProblemMemberReserved {
                        members,
                        member: name,
                    });
                }
                let sent = required.contains(&name);
                Ok((name, schema, sent))
            })
            .collect()
    }
}

/// Whether `object` is a plain object naming its members as properties, the
/// one shape whose members a problem can carry beside its own.
///
/// Its `additionalProperties` may be absent, `true`, or `false`: only the
/// properties are read, so a `deny_unknown_fields` struct, which the `Schema`
/// derive closes, declares the members it names.
fn describes_members(object: &SchemaObject) -> bool {
    object.ty == Some(TypeSet::One(SchemaType::Object))
        && object.reference.is_none()
        && object.dynamic_ref.is_none()
        && object.all_of.is_none()
        && object.any_of.is_none()
        && object.one_of.is_none()
        && object.not.is_none()
        && object.if_schema.is_none()
        && object.pattern_properties.is_empty()
        && object
            .additional_properties
            .as_deref()
            .is_none_or(|additional| matches!(additional, OpenApiSchema::Bool(_)))
}

/// `M`'s members for `cx`, as the object they serialize to.
fn produce<M: ProblemMembers>(
    cx: &ProblemCx<'_>,
) -> Result<serde_json::Map<String, Value>, Unsent> {
    match serde_json::to_value(M::for_problem(cx)) {
        Ok(Value::Object(members)) => Ok(members),
        Ok(_) | Err(_) => Err(Unsent),
    }
}

/// The request head a problem answers, borrowed from wherever it is held.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Head<'a> {
    method: &'a Method,
    uri: &'a Uri,
    headers: &'a HeaderMap,
    extensions: &'a Extensions,
}

impl<'a> From<&'a Parts> for Head<'a> {
    fn from(parts: &'a Parts) -> Self {
        Self {
            method: &parts.method,
            uri: &parts.uri,
            headers: &parts.headers,
            extensions: &parts.extensions,
        }
    }
}

impl<'a> From<&'a Request> for Head<'a> {
    fn from(request: &'a Request) -> Self {
        Self {
            method: request.method(),
            uri: request.uri(),
            headers: request.headers(),
            extensions: request.extensions(),
        }
    }
}

/// A problem Kynos owns, rendered with the members the router installed.
#[derive(Debug)]
pub(crate) enum Rendered {
    /// The answer, carrying them; its caller may write fields of its own onto
    /// it.
    Kept(Response),
    /// The 500 sent in its place, where they cannot be sent; nothing more is
    /// written onto it.
    Refused(Response),
}

impl Rendered {
    /// Whichever response is sent.
    pub(crate) fn into_response(self) -> Response {
        match self {
            Self::Kept(response) | Self::Refused(response) => response,
        }
    }
}

/// So an interceptor's chain can settle to one it rendered before the
/// interceptor took the request.
impl IntoResponse for Rendered {
    fn into_response(self) -> Response {
        Rendered::into_response(self)
    }
}

/// `problem`, rendered for the request `head` with the members `produce`
/// yields, where the router installed any.
pub(crate) fn render(
    mut problem: Problem,
    origin: ProblemOrigin,
    head: Head<'_>,
    produce: Option<Produce>,
) -> Rendered {
    if let Some(produce) = produce {
        let cx = ProblemCx {
            problem: &problem,
            origin,
            head,
        };
        let Ok(members) = produce(&cx) else {
            return Rendered::Refused(
                Problem::new(StatusCode::INTERNAL_SERVER_ERROR).into_response(),
            );
        };
        add(&mut problem.extensions, members);
    }

    Rendered::Kept(problem.into_response())
}

/// `problem`, rendered as the answer to the request whose head is `head`, with
/// the members the router that routed it installed.
pub(crate) fn served(problem: Problem, origin: ProblemOrigin, head: Head<'_>) -> Rendered {
    let produce = Installed::on(head.extensions);

    render(problem, origin, head, produce)
}

/// The members the router installed, on a routed request it inserts them
/// into; absent from every request where none were installed.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Installed(pub(crate) Produce);

impl Installed {
    /// What the router that routed a request with `extensions` installed.
    pub(crate) fn on(extensions: &Extensions) -> Option<Produce> {
        extensions.get::<Self>().map(|Self(produce)| *produce)
    }
}

/// Adds `members` beside what `extensions` already holds; a member the problem
/// already sends keeps its own value.
fn add(extensions: &mut BTreeMap<String, Value>, members: serde_json::Map<String, Value>) {
    for (name, value) in members {
        extensions.entry(name).or_insert(value);
    }
}
