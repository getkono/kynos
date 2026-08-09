//! Variables captured from the path template.

use crate::{
    error::Rejection,
    extract::{FromRequestParts, describe::Describe},
    http::Parts,
    router::OperationCx,
    schema::{Registry, Schema},
};

/// Variables captured from the path template.
///
/// `T` derives `PathParams`, and its field names are checked against the route
/// template at compile time — a mismatch is a compile error, not a runtime 500,
/// which is the failure mode every other Rust framework has here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Path<T>(pub T);

/// A group of path parameters.
///
/// Derived, never implemented by hand. [`NAMES`](PathParams::NAMES) is what the
/// route attribute compares against the path template.
pub trait PathParams: Sized {
    /// The parameter names, in declaration order.
    const NAMES: &'static [&'static str];

    /// Decodes the named captures from a matched route.
    fn decode(values: &[(&str, &str)]) -> Result<Self, Rejection> {
        let _ = values;
        todo!()
    }

    /// Encodes this value for a typed endpoint URI.
    fn encode(&self) -> Vec<(&'static str, String)> {
        todo!()
    }

    /// Describes each captured value as an OpenAPI path parameter.
    fn parameters(registry: &mut Registry) -> Vec<kynos_openapi::Parameter> {
        let _ = registry;
        todo!()
    }
}

impl<C: Sync, T: PathParams + Send> FromRequestParts<C> for Path<T> {
    type Rejection = Rejection;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (parts, context);
        todo!()
    }
}

impl<T: PathParams + Schema> Describe for Path<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
        todo!()
    }
}
