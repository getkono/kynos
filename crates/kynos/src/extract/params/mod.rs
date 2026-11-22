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
    error::rejection::{Input, constraint_responses},
    router::operation::OperationCx,
    schema::{constraints::ConstraintProblem, registry::Registry},
};

/// Adds each Parameter Object a group's `parameters` builds to `operation`,
/// and the response of each problem in `named` for a broken bound.
fn describe_parameters(
    operation: &mut OperationCx<'_>,
    parameters: fn(&mut Registry) -> Vec<kynos_openapi::Parameter>,
    named: &[ConstraintProblem],
) {
    for parameter in parameters(operation.registry()) {
        operation.add_parameter(parameter);
    }
    describe_constraints(operation, named);
}

/// Declares each problem in `named` at the status it answers with: at 400 it
/// joins the `about:blank` one a parameter rejection declares, and a 422 is
/// its own.
fn describe_constraints(operation: &mut OperationCx<'_>, named: &[ConstraintProblem]) {
    let named = constraint_responses(operation.registry(), Input::Parameter, named, true);
    operation.add_responses(&named);
}
