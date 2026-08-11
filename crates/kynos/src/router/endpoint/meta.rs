//! The compile-time facts a route attribute knows about an operation.

use crate::middleware::catch_panic::PanicPolicy;

/// The compile-time facts a route attribute knows about an operation.
///
/// The attribute macros expand a handler into a zero-sized type implementing
/// this, which is what `routes!` collects. Everything here is `const`, so the
/// checks that depend on it — that path template variables match the handler's
/// path parameters, that no `operationId` repeats — happen during compilation
/// rather than at startup.
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
    ///
    /// The assertion reads this constant rather than rebuilding the list, so
    /// what the description will say and what the handler destructures are
    /// checked against one source.
    const PATH_VARIABLES: &'static [&'static str];

    /// The operation identifier.
    ///
    /// Defaults to the handler's module path and name, which is unique by
    /// construction.
    const OPERATION_ID: &'static str;

    /// The first line of the handler's doc comment.
    const SUMMARY: Option<&'static str>;

    /// The rest of the handler's doc comment.
    const DESCRIPTION: Option<&'static str>;

    /// Whether the handler carried `#[deprecated]`.
    const DEPRECATED: bool;

    /// The tags the route attribute named, in declaration order.
    ///
    /// Empty when it named none. This is the fourth and innermost of the four
    /// places a tag can be applied — [`Router::tag`](crate::Router::tag),
    /// [`Group::tag`](crate::router::group::Group::tag) and
    /// [`EndpointBuilder::tag`](crate::router::endpoint::builder::EndpointBuilder::tag)
    /// are the other three — and the only one readable without building a
    /// router, since it is a fact about the operation rather than about what
    /// encloses it.
    ///
    /// A slice rather than an `Option`, because the enclosing levels contribute
    /// their own and the Operation Object's `tags` is an array either way.
    const TAGS: &'static [&'static str];
}
