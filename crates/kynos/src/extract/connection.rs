//! Inputs describing the connection rather than the API.
//!
//! Both contribute nothing to the description, which is the point: they are
//! properties of how a request arrived, not of the contract it is part of.

use core::convert::Infallible;

use crate::{
    extract::{FromRequestParts, describe::Describe},
    http::Parts,
    router::operation::OperationCx,
};

/// The path template this request matched.
///
/// Exactly the `paths` key from the description, which makes it the correct
/// label for a metric — unlike the concrete URI, it has bounded cardinality.
/// Contributes nothing to the description.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MatchedPath(pub &'static str);

/// The peer address of the connection this request arrived on.
///
/// Contributes nothing to the description: it is a property of the connection,
/// not of the API.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnectInfo(pub std::net::SocketAddr);

/// Infallible because a route has already matched by the time an argument is
/// built: the template that matched is what this returns, so there is no state
/// in which it is absent.
impl<C: Sync> FromRequestParts<C> for MatchedPath {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (parts, context);
        todo!()
    }
}

impl Describe for MatchedPath {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
    }
}

/// Infallible because a request that reached a handler arrived on a connection,
/// and the peer address is a property of that connection rather than of
/// anything the client sent.
impl<C: Sync> FromRequestParts<C> for ConnectInfo {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (parts, context);
        todo!()
    }
}

impl Describe for ConnectInfo {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
    }
}
