//! The type, title and extension members an application gives the problems
//! Kynos renders itself.
//!
//! A rejection, a guard's refusal and a router fallback are Kynos's types,
//! with shapes the application cannot reach. [`Router::problems`] installs one
//! [`ProblemPolicy`], which names the type and title each of those problems
//! carries in place of `about:blank`, and both produces the members each one
//! carries and declares them on every response describing one, so the
//! description cannot drift from the wire (RFC 9457 section 3.2).
//!
//! ```
//! # #[cfg(feature = "macros")]
//! # {
//! use kynos::{
//!     Router, Schema,
//!     error::members::{ProblemCx, ProblemIdentity, ProblemOrigin, ProblemPolicy},
//!     http::StatusCode,
//! };
//!
//! /// A stable code a client localizes offline.
//! #[derive(Schema, serde::Serialize)]
//! struct Coded {
//!     code: String,
//! }
//!
//! struct AppProblems;
//!
//! impl ProblemPolicy for AppProblems {
//!     type Members = Coded;
//!
//!     fn identity(origin: ProblemOrigin, status: StatusCode) -> Option<ProblemIdentity> {
//!         match (origin, status) {
//!             (_, StatusCode::UNAUTHORIZED) => Some(ProblemIdentity::new(
//!                 "urn:example:problem:unauthorized",
//!                 "Unauthorized",
//!             )),
//!             _ => None,
//!         }
//!     }
//!
//!     fn members(cx: &ProblemCx<'_>) -> Coded {
//!         let code = match cx.origin() {
//!             ProblemOrigin::Authentication => "auth",
//!             ProblemOrigin::Routing => "route",
//!             _ => "request",
//!         };
//!         Coded { code: format!("{code}-{}", cx.status().as_u16()) }
//!     }
//! }
//!
//! let router = Router::<()>::new().problems::<AppProblems>();
//! # let _ = router;
//! # }
//! ```
//!
//! [`Router::problems`]: crate::router::Router::problems

use std::collections::BTreeMap;

use kynos_openapi::{
    Schema as OpenApiSchema, SchemaObject,
    model::schema::types::{SchemaType, TypeSet},
};
use serde_json::Value;

use crate::{
    error::problem::{ABOUT_BLANK, Problem, RESERVED},
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
    /// A credential: [`AuthRejection`](crate::error::rejection::AuthRejection),
    /// as a [`GuardRejection`](crate::error::rejection::GuardRejection) sends it.
    Authentication,
    /// The router, for a request no operation answers: the 404, 405 and 501
    /// under [`FallbackPolicy::Problem`](crate::router::policy::FallbackPolicy::Problem),
    /// and a CORS preflight's answer to an `OPTIONS` that is not one.
    Routing,
    /// The request's `Range`, which no part of the file an asset endpoint
    /// serves satisfies:
    /// [`RangeRejection`](crate::error::rejection::RangeRejection).
    ByteRange,
    /// An interceptor Kynos ships, refusing with its own `Short`:
    /// [`BodySizeExceeded`](crate::middleware::limits::body_size::BodySizeExceeded),
    /// [`RateLimited`](crate::middleware::rate_limit::refusal::RateLimited)
    /// and the rest. Rendered after the interceptor gave the request away, so
    /// [`ProblemCx`] holds no request head for it. Not named `Interceptor`,
    /// which the trait is, for the reason above.
    Refusal,
    /// A panic a recovery branch caught, answered with a 500. Rendered after
    /// the request moved into the future that unwound, so [`ProblemCx`]
    /// holds no request head for it.
    Panic,
}

/// What the router knows when it renders a problem it owns.
///
/// Read by [`ProblemPolicy::members`]. The request is the head the operation
/// saw, so an extension an interceptor inserted, such as a request ID, is
/// there to read; every accessor reading it answers `None` for
/// [`ProblemOrigin::Refusal`] and [`ProblemOrigin::Panic`], which are
/// rendered after the request is gone.
#[derive(Debug)]
pub struct ProblemCx<'a> {
    problem: &'a Problem,
    origin: ProblemOrigin,
    head: Option<Head<'a>>,
}

impl<'a> ProblemCx<'a> {
    /// The problem as Kynos built it, carrying the identity the policy gave
    /// it, before any member is added.
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

    /// The request's method, where its head is still in hand.
    #[must_use]
    pub fn method(&self) -> Option<&'a Method> {
        self.head.map(|head| head.method)
    }

    /// The request's target, where its head is still in hand.
    #[must_use]
    pub fn uri(&self) -> Option<&'a Uri> {
        self.head.map(|head| head.uri)
    }

    /// The request's header fields, where its head is still in hand.
    #[must_use]
    pub fn headers(&self) -> Option<&'a HeaderMap> {
        self.head.map(|head| head.headers)
    }

    /// The request's extensions, as the interceptors outside the refusal left
    /// them, where its head is still in hand.
    #[must_use]
    pub fn extensions(&self) -> Option<&'a Extensions> {
        self.head.map(|head| head.extensions)
    }
}

/// What every problem Kynos renders itself carries, installed with
/// [`Router::problems`](crate::router::Router::problems).
///
/// Two parts, each sent and declared from one place so the description
/// cannot drift from the wire:
///
/// - [`identity`](ProblemPolicy::identity) names the `type` and `title` of a
///   problem Kynos would otherwise send as `about:blank`. It reads only where
///   the problem came from and its status, which is what each response's
///   description knows too, so the `type` a response declares is the `type`
///   it sends. A problem that already names a type keeps it: one an
///   interceptor's [`ProblemType`](crate::error::problem::ProblemType) or an
///   authorizer's [`forbidden_as`](crate::error::rejection::AuthRejection::forbidden_as)
///   chose is the more specific answer.
/// - [`members`](ProblemPolicy::members) produces the extension members each
///   problem carries, sent through `Serialize` and declared by `Schema`, as a
///   `#[problem(extension)]` field is. `()` sends and declares none.
///   Otherwise the schema must be an object's, naming each member as a
///   property: a struct deriving both, with no `#[serde(flatten)]`;
///   `#[serde(deny_unknown_fields)]` may close it, since only its properties
///   are read. A member may not be named like one RFC 9457 registers (`type`,
///   `title`, `status`, `detail`, `instance`) or like `errors`, which a schema
///   failure sends; the router refuses both while it is described. A value
///   whose `Serialize` fails, or does not serialize as an object, answers 500
///   in place of the problem, as an `ApiError` member does.
///
/// An application's own `#[derive(ApiError)]` type is not Kynos's to render,
/// so neither part reaches it: it names its own type, title and members.
pub trait ProblemPolicy: 'static {
    /// The extension members every problem carries; `()` for none.
    type Members: Schema + serde::Serialize + 'static;

    /// The type and title a problem from `origin` answering with `status`
    /// carries in place of `about:blank`, or `None` to keep `about:blank` and
    /// the status's reason phrase.
    ///
    /// Defaults to `None` for every problem.
    #[must_use]
    fn identity(origin: ProblemOrigin, status: StatusCode) -> Option<ProblemIdentity> {
        let _ = (origin, status);
        None
    }

    /// The members the problem `cx` describes carries.
    fn members(cx: &ProblemCx<'_>) -> Self::Members;
}

/// The `type` and `title` a [`ProblemPolicy`] gives a problem (RFC 9457
/// sections 3.1.1 and 3.1.3).
///
/// `'static`, as the description declares both: the `type` as the constant
/// the response's schema narrows to, and the `title` as that branch's
/// summary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProblemIdentity {
    type_uri: &'static str,
    title: &'static str,
}

impl ProblemIdentity {
    /// The problem type `type_uri`, summarized by `title`.
    ///
    /// The title should not change from occurrence to occurrence of the type
    /// (RFC 9457 section 3.1.3), which a `'static` string keeps.
    #[must_use]
    pub const fn new(type_uri: &'static str, title: &'static str) -> Self {
        Self { type_uri, title }
    }

    /// The URI identifying the problem type.
    #[must_use]
    pub const fn type_uri(&self) -> &'static str {
        self.type_uri
    }

    /// The problem type's short summary.
    #[must_use]
    pub const fn title(&self) -> &'static str {
        self.title
    }
}

/// A policy's [`identity`](ProblemPolicy::identity), erased.
pub(crate) type Identify = fn(ProblemOrigin, StatusCode) -> Option<ProblemIdentity>;

/// The members one problem carries, produced and serialized.
pub(crate) type Produce = fn(&ProblemCx<'_>) -> Result<serde_json::Map<String, Value>, Unsent>;

/// Members that could not be sent: a failed or non-object serialization.
#[derive(Debug)]
pub(crate) struct Unsent;

/// What a [`ProblemPolicy`] does to a problem as it is rendered: the identity
/// it gives it and the members it adds.
#[derive(Debug)]
pub(crate) struct Rendering {
    pub(crate) identify: Identify,
    pub(crate) produce: Produce,
}

/// A [`Rendering`], as everything carrying one holds it: a pointer wide, since
/// every interceptor chain carries one whether a policy is installed or not.
pub(crate) type Policy = &'static Rendering;

/// One [`ProblemPolicy`], erased: how it renders a problem and how it declares
/// what it renders.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hook {
    pub(crate) policy: Policy,
    describe: fn(&mut Registry) -> OpenApiSchema,
    /// Whether its members are `()`, which sends and declares none.
    memberless: bool,
    name: &'static str,
}

impl Hook {
    /// `P`, erased.
    pub(crate) fn of<P: ProblemPolicy>() -> Self {
        Self {
            policy: const {
                &Rendering {
                    identify: P::identity,
                    produce: produce::<P>,
                }
            },
            describe: P::Members::schema,
            memberless: memberless::<P>(),
            name: std::any::type_name::<P::Members>(),
        }
    }

    /// Declares what the policy `hook` erases renders, where one is
    /// installed, on `registry` for every problem Kynos renders to read; or
    /// the violation refusing its members.
    pub(crate) fn declare(hook: Option<Self>, registry: &mut Registry) -> Option<RouterError> {
        let hook = hook?;
        match hook.declared(registry) {
            Ok(members) => {
                registry.declare_problem_policy(members, hook.policy.identify);
                None
            }
            Err(refused) => Some(refused),
        }
    }

    /// Each member the type declares: its name, its schema, and whether every
    /// problem sends it.
    ///
    /// Refuses a schema that is not an object's naming its members, and a
    /// member named like one a problem already sends. `()` declares none.
    pub(crate) fn declared(
        &self,
        registry: &mut Registry,
    ) -> Result<Vec<(String, OpenApiSchema, bool)>, RouterError> {
        if self.memberless {
            return Ok(Vec::new());
        }
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

/// Whether `P` adds no members: its `Members` is `()`.
fn memberless<P: ProblemPolicy>() -> bool {
    std::any::TypeId::of::<P::Members>() == std::any::TypeId::of::<()>()
}

/// `P`'s members for `cx`, as the object they serialize to; none for `()`.
fn produce<P: ProblemPolicy>(cx: &ProblemCx<'_>) -> Result<serde_json::Map<String, Value>, Unsent> {
    if memberless::<P>() {
        return Ok(serde_json::Map::new());
    }
    match serde_json::to_value(P::members(cx)) {
        Ok(Value::Object(members)) => Ok(members),
        Ok(_) | Err(_) => Err(Unsent),
    }
}

/// The type URI and summary the branch of a problem Kynos renders from
/// `origin` with `status` declares: the identity the installed policy gives
/// it where `served` says the router renders it, and `about:blank` with no
/// summary otherwise.
pub(crate) fn declared_identity(
    registry: &Registry,
    origin: ProblemOrigin,
    status: StatusCode,
    served: bool,
) -> (Option<&'static str>, Option<&'static str>) {
    match registry.problem_identity(origin, status).filter(|_| served) {
        Some(identity) => (Some(identity.type_uri), Some(identity.title)),
        None => (None, None),
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

/// A problem Kynos owns, rendered under the policy the router installed.
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

/// `problem`, rendered for the request `head` under `policy`, where the
/// router installed one: an `about:blank` problem takes the identity it
/// gives, then the problem carries the members it yields.
pub(crate) fn render(
    problem: Problem,
    origin: ProblemOrigin,
    head: Head<'_>,
    policy: Option<Policy>,
) -> Rendered {
    render_for(problem, origin, Some(head), policy)
}

/// An interceptor's refusal `problem`, rendered under `policy` with no
/// request head to read, and `finish` writing the refusal's own fields onto
/// it; the 500 that replaces it where its members cannot be sent gets
/// nothing more.
pub(crate) fn refused(
    problem: Problem,
    policy: Option<Policy>,
    finish: impl FnOnce(&mut Response),
) -> Response {
    match render_for(problem, ProblemOrigin::Refusal, None, policy) {
        Rendered::Kept(mut response) => {
            finish(&mut response);
            response
        }
        Rendered::Refused(refused) => refused,
    }
}

/// The 500 a recovered panic becomes, rendered under `policy`; it never
/// exposes the payload.
pub(crate) fn panicked(policy: Option<Policy>) -> Response {
    let problem = Problem::new(StatusCode::INTERNAL_SERVER_ERROR);

    render_for(problem, ProblemOrigin::Panic, None, policy).into_response()
}

/// [`render`], for a problem whose request head may already be gone.
fn render_for(
    mut problem: Problem,
    origin: ProblemOrigin,
    head: Option<Head<'_>>,
    policy: Option<Policy>,
) -> Rendered {
    if let Some(policy) = policy {
        identify(&mut problem, origin, policy.identify);
        let cx = ProblemCx {
            problem: &problem,
            origin,
            head,
        };
        let Ok(members) = (policy.produce)(&cx) else {
            return Rendered::Refused(
                Problem::new(StatusCode::INTERNAL_SERVER_ERROR).into_response(),
            );
        };
        add(&mut problem.extensions, members);
    }

    Rendered::Kept(problem.into_response())
}

/// Gives `problem` the identity `identify` names for it, where it names one
/// and the problem names no type of its own.
fn identify(problem: &mut Problem, origin: ProblemOrigin, identify: Identify) {
    if problem.type_uri != ABOUT_BLANK {
        return;
    }
    if let Some(identity) = identify(origin, problem.status) {
        problem.type_uri = identity.type_uri.into();
        problem.title = identity.title.into();
    }
}

/// `problem`, rendered as the answer to the request whose head is `head`,
/// under the policy the router that routed it installed.
pub(crate) fn served(problem: Problem, origin: ProblemOrigin, head: Head<'_>) -> Rendered {
    let policy = Installed::on(head.extensions);

    render(problem, origin, head, policy)
}

/// The policy the router installed, on a routed request it inserts it into;
/// absent from every request where none was installed.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Installed(pub(crate) Policy);

impl Installed {
    /// What the router that routed a request with `extensions` installed.
    pub(crate) fn on(extensions: &Extensions) -> Option<Policy> {
        extensions.get::<Self>().map(|Self(policy)| *policy)
    }
}

/// Adds `members` beside what `extensions` already holds; a member the problem
/// already sends keeps its own value.
fn add(extensions: &mut BTreeMap<String, Value>, members: serde_json::Map<String, Value>) {
    for (name, value) in members {
        extensions.entry(name).or_insert(value);
    }
}
