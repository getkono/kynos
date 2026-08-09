//! Responses whose status is fixed by their type.

use crate::{
    http::Response,
    response::{IntoResponse, Responses},
    schema::Registry,
};

/// A 204 No Content response.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NoContent;

/// A 201 Created response carrying the created representation.
///
/// The `Location` header is required rather than optional: a 201 without one
/// tells a client something was created but not where, which is rarely what
/// anybody wants and is trivial to forget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Created<T> {
    /// The created representation.
    pub body: T,
    /// Where the new resource lives.
    pub location: String,
}

impl<T> Created<T> {
    /// Creates a 201 response for a resource at `location`.
    pub fn at(location: impl Into<String>, body: T) -> Self {
        Self {
            body,
            location: location.into(),
        }
    }
}

/// A 202 Accepted response for work that has not finished.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Accepted<T> {
    /// A representation of the accepted work, typically a job handle.
    pub body: T,
}

impl<T> Accepted<T> {
    /// Creates a 202 response carrying the accepted work representation.
    pub fn new(body: T) -> Self {
        Self { body }
    }
}

/// A redirect with a status fixed at compile time.
///
/// `CODE` must be one of 301, 302, 303, 307 or 308; anything else fails to
/// compile. That rules out the most common redirect bug, which is using 302
/// where 307 was meant and silently changing the method on replay.
///
/// ```compile_fail
/// fn response<T: kynos::response::IntoResponse>(value: T) { drop(value); }
/// response(kynos::response::status::Redirect::<304>::to("/cached"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Redirect<const CODE: u16> {
    /// The target of the redirect.
    pub location: String,
}

impl<const CODE: u16> Redirect<CODE> {
    /// Redirects to `location`.
    pub fn to(location: impl Into<String>) -> Self {
        Self {
            location: location.into(),
        }
    }
}

/// A compile-time proof that a redirect status is supported.
///
/// Implemented by Kynos for `()` and the five redirect statuses accepted by
/// [`Redirect`]. Downstream crates cannot add implementations because both the
/// trait and `()` are foreign there.
pub trait ValidRedirectCode<const CODE: u16> {}

impl ValidRedirectCode<301> for () {}
impl ValidRedirectCode<302> for () {}
impl ValidRedirectCode<303> for () {}
impl ValidRedirectCode<307> for () {}
impl ValidRedirectCode<308> for () {}

impl IntoResponse for NoContent {
    fn into_response(self) -> Response {
        todo!()
    }
}

impl Responses for NoContent {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}

impl<T: IntoResponse> IntoResponse for Created<T> {
    fn into_response(self) -> Response {
        todo!()
    }
}

impl<T: Responses> Responses for Created<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}

impl<T: IntoResponse> IntoResponse for Accepted<T> {
    fn into_response(self) -> Response {
        todo!()
    }
}

impl<T: Responses> Responses for Accepted<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}

impl<const CODE: u16> IntoResponse for Redirect<CODE>
where
    (): ValidRedirectCode<CODE>,
{
    fn into_response(self) -> Response {
        todo!()
    }
}

impl<const CODE: u16> Responses for Redirect<CODE>
where
    (): ValidRedirectCode<CODE>,
{
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}
