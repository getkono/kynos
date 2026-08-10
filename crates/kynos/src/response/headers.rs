//! Declaring response headers as part of the return type.

use crate::{
    extract::params::header::HeaderParams,
    http::Response,
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};

/// A response carrying declared headers alongside its body.
///
/// `H` derives `Headers`, so each header appears in `Response.headers` with its
/// own schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WithHeaders<T, H> {
    /// The response body.
    pub body: T,
    /// The declared headers.
    pub headers: H,
}

impl<T, H> WithHeaders<T, H> {
    /// Attaches a derived header group to a response body.
    pub fn new(body: T, headers: H) -> Self {
        Self { body, headers }
    }
}

impl<T, H> IntoResponse for WithHeaders<T, H>
where
    T: IntoResponse,
    H: HeaderParams,
{
    fn into_response(self) -> Response {
        todo!()
    }
}

impl<T, H> Responses for WithHeaders<T, H>
where
    T: Responses,
    H: HeaderParams,
{
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}
