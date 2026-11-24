//! The compile-time facts a route attribute knows about an operation.

use crate::{middleware::catch_panic::PanicPolicy, router::operation::DeclaredTag};

/// The compile-time facts a route attribute knows about an operation.
///
/// The attribute macros expand a handler into a zero-sized type implementing
/// this, which is what `routes!` collects. Everything here is `const`, so the
/// check that path template variables match the handler's path parameters
/// happens during compilation rather than at startup. A repeated
/// `operationId` is not caught here: validating or building the router
/// refuses it.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not an operation",
    label = "not an operation",
    note = "`routes!` takes handlers carrying a route attribute — `#[kynos::get(\"/path\")]` and \
            its siblings. A plain function has none, and a function item cannot be named as a \
            type, so passing one fails before this bound is even checked"
)]
pub trait EndpointMeta {
    /// How this endpoint handles a panic while executing its operation.
    type PanicPolicy: PanicPolicy;

    /// The HTTP method, spelled as it appears on the wire.
    const METHOD: &'static str;

    /// The path template, relative to any enclosing group.
    const PATH: &'static str;

    /// The variable names appearing in [`PATH`](EndpointMeta::PATH).
    ///
    /// Compared against `PathParams::NAMES` by a const assertion in the
    /// expansion, so a handler whose parameters do not match its path is a
    /// compile error rather than a runtime 500.
    const PATH_VARIABLES: &'static [&'static str];

    /// The operation identifier, as the attribute emitted it.
    ///
    /// Defaults to the handler function's name; the attribute's `operation_id`
    /// argument overrides it. The one published, unless
    /// [`OPERATION_ID_DEFAULTED`](EndpointMeta::OPERATION_ID_DEFAULTED) and the
    /// router recases it.
    const OPERATION_ID: &'static str;

    /// Whether [`OPERATION_ID`](EndpointMeta::OPERATION_ID) is the handler's
    /// name, which the route attribute defaulted it to, rather than one its
    /// `operation_id` argument wrote.
    ///
    /// A defaulted identifier is one
    /// [`Router::operation_id_case`](crate::Router::operation_id_case) recases;
    /// a written one is published exactly as written, and a
    /// [`Link`](crate::extract::link::Link) finds its target either way.
    /// Defaults to `false`, so an implementation written before it existed
    /// keeps the identifier it wrote.
    const OPERATION_ID_DEFAULTED: bool = false;

    /// The first line of the handler's doc comment.
    const SUMMARY: Option<&'static str>;

    /// The rest of the handler's doc comment.
    const DESCRIPTION: Option<&'static str>;

    /// Whether the handler carried `#[deprecated]`.
    const DEPRECATED: bool;

    /// Whether the handler carried `#[kynos::public]`, declaring that it takes
    /// no guard on purpose, as
    /// [`EndpointBuilder::public`](crate::router::endpoint::builder::EndpointBuilder::public)
    /// does.
    ///
    /// Defaults to `false`, so an implementation written before it existed
    /// declares nothing it did not.
    const PUBLIC: bool = false;

    /// The tags the route attribute named, in declaration order.
    ///
    /// Empty when it named none. The innermost of the four places a tag can be
    /// applied, beside [`Router::tag`](crate::Router::tag),
    /// [`Group::tag`](crate::router::group::Group::tag) and
    /// [`EndpointBuilder::tag`](crate::router::endpoint::builder::EndpointBuilder::tag),
    /// and the only one readable without building a router.
    const TAGS: &'static [DeclaredTag];

    /// The `operationId` of the `HEAD` this `GET` describes beside itself, as
    /// [`EndpointBuilder::describe_head`](crate::router::endpoint::builder::EndpointBuilder::describe_head)
    /// does; `None` where it describes none.
    ///
    /// Set by the route attribute's `head` argument, which only `GET` takes.
    /// Defaults to `None`, so an implementation written before it existed
    /// describes nothing it did not.
    const HEAD_OPERATION_ID: Option<&'static str> = None;

    /// Whether [`HEAD_OPERATION_ID`](EndpointMeta::HEAD_OPERATION_ID) was
    /// derived from the handler's name by a bare `head`, rather than named by
    /// `head = "..."`.
    ///
    /// Read as [`OPERATION_ID_DEFAULTED`](EndpointMeta::OPERATION_ID_DEFAULTED)
    /// is, and defaults to `false` for the same reason.
    const HEAD_OPERATION_ID_DEFAULTED: bool = false;
}

/// How a route attribute renders its operation's URI against a path template.
///
/// The attribute implements this beside [`EndpointMeta`], with
/// [`Params`](EndpointUri::Params) exactly the path and query types its handler
/// extracts, so a [`Link`](crate::extract::link::Link) to the operation takes
/// the same arguments its `relative_uri` does.
pub trait EndpointUri: EndpointMeta {
    /// What the URI is built from: `()`, the handler's `Path<P>` group, its
    /// `Query<Q>` group, or `(P, Q)` when it extracts both.
    type Params;

    /// Renders `params` into `template`, a path template with exactly
    /// [`PATH_VARIABLES`](EndpointMeta::PATH_VARIABLES) for variables,
    /// percent-encoding each value.
    fn render(template: &str, params: Self::Params) -> crate::http::Uri;
}
