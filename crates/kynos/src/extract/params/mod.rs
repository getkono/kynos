//! Inputs drawn from the request head, each describing itself as an OpenAPI
//! Parameter Object.
//!
//! One module per parameter location, each holding the wrapper a handler
//! receives and, for named parameters, the derived trait describing the group.

pub mod header;
pub mod path;
pub mod query;

#[cfg(feature = "openapi32")]
pub mod querystring;

#[cfg(feature = "cookie")]
pub mod cookie;

use crate::{router::operation::OperationCx, schema::registry::Registry};

/// Adds each Parameter Object a group's `parameters` builds to `operation`.
fn describe_parameters(
    operation: &mut OperationCx<'_>,
    parameters: fn(&mut Registry) -> Vec<kynos_openapi::Parameter>,
) {
    for parameter in parameters(operation.registry()) {
        operation.add_parameter(parameter);
    }
}
