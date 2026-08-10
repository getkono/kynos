//! What a route attribute expands into.

use kynos_openapi::{Method, PathTemplate};

use crate::{
    handler::Handler,
    middleware::catch_panic::PanicPolicy,
    router::endpoint::{EndpointBuilder, EndpointMeta},
};

/// Builds an endpoint from a route attribute's compile-time facts.
///
/// `M` is the marker type the attribute emitted, carrying [`EndpointMeta`];
/// `handler` is the function of the same name. The attribute emits a *braced*
/// struct, so one identifier names the marker in type position and the function
/// in value position — which is what lets `routes![get_user]` pass both without
/// naming the function's unnameable type.
///
/// # Panics
///
/// Panics if the marker's method or path is not one the attribute could have
/// emitted. Reaching that means `EndpointMeta` was implemented by hand with
/// values the attribute's own compile-time checks would have rejected.
pub fn from_meta<C, M, H, A>(handler: H) -> EndpointBuilder<C, H, A, M::PanicPolicy>
where
    C: Send + Sync + 'static,
    M: EndpointMeta,
    H: Handler<C, A>,
    M::PanicPolicy: PanicPolicy,
{
    let method = Method::from_wire_str(M::METHOD)
        .expect("a route attribute only emits a method with a Path Item field");
    let path =
        PathTemplate::parse(M::PATH).expect("a route attribute only emits a valid path template");

    let mut builder =
        EndpointBuilder::with_policy(method, path, handler).operation_id(M::OPERATION_ID);
    if let Some(summary) = M::SUMMARY {
        builder = builder.summary(summary);
    }
    if let Some(description) = M::DESCRIPTION {
        builder = builder.description(description);
    }
    if M::DEPRECATED {
        builder = builder.deprecated();
    }
    builder
}
