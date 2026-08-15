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
///
/// # Where the value comes from
///
/// The router inserts the matched template into
/// [`Parts::extensions`](crate::http::Parts) as a `MatchedPath`, before any
/// argument is built. Extracting one is reading that back, which is why it
/// cannot fail: the insertion happens on the same code path as the match.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MatchedPath(pub &'static str);

/// The peer address of the connection this request arrived on.
///
/// Contributes nothing to the description: it is a property of the connection,
/// not of the API.
///
/// # Where the value comes from
///
/// The server inserts a `ConnectInfo` into
/// [`Parts::extensions`](crate::http::Parts) when it hands a request to the
/// service, so extracting one reads back what the accept loop already knew.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnectInfo(pub std::net::SocketAddr);

/// Infallible because a route has already matched by the time an argument is
/// built: the template that matched is what this returns, so there is no state
/// in which it is absent.
impl<C: Sync> FromRequestParts<C> for MatchedPath {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        Ok(parts
            .extensions
            .get::<Self>()
            .cloned()
            .expect("the router records the matched path template before building an argument"))
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

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        Ok(*parts
            .extensions
            .get::<Self>()
            .expect("the server records the peer address before handing over a request"))
    }
}

impl Describe for ConnectInfo {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
    }
}
