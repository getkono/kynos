//! Inputs describing the connection rather than the API.
//!
//! Both contribute nothing to the description, which is the point: they are
//! properties of how a request arrived, not of the contract it is part of.

use crate::{
    error::rejection::Rejection,
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

impl<C: Sync> FromRequestParts<C> for MatchedPath {
    type Rejection = Rejection;

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

impl<C: Sync> FromRequestParts<C> for ConnectInfo {
    type Rejection = Rejection;

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
