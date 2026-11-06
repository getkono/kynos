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

use crate::{
    error::rejection::constraint_responses, http::StatusCode, router::operation::OperationCx,
    schema::registry::Registry,
};

/// Adds each Parameter Object a group's `parameters` builds to `operation`,
/// and the 400 of each problem type `named` for a broken bound.
fn describe_parameters(
    operation: &mut OperationCx<'_>,
    parameters: fn(&mut Registry) -> Vec<kynos_openapi::Parameter>,
    named: &[&'static str],
) {
    for parameter in parameters(operation.registry()) {
        operation.add_parameter(parameter);
    }
    describe_constraints(operation, named);
}

/// Declares the 400 of each problem type in `named`, which joins the
/// `about:blank` one a parameter rejection declares.
fn describe_constraints(operation: &mut OperationCx<'_>, named: &[&'static str]) {
    let named = constraint_responses(operation.registry(), StatusCode::BAD_REQUEST, named);
    operation.add_responses(&named);
}
