//! Responses whose status is fixed by their type.

use crate::{
    http::Response,
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};

/// Where a response points a client next.
///
/// The value of a `Location` header, and the reason it is a type rather than a
/// `String`: a route attribute's
/// `relative_uri` returns an [`http::Uri`], and neither that
/// type nor `String` belongs to Kynos, so no `From` between them can be written
/// here. Naming the concept is what lets both spellings arrive without a
/// conversion at every call site.
///
/// ```
/// use kynos::response::status::Location;
///
/// let literal = Location::from("/users/42");
/// let owned = Location::from(String::from("/users/42"));
/// let parsed = Location::from("/users/42".parse::<kynos::http::Uri>().unwrap());
/// assert_eq!(literal, owned);
/// assert_eq!(literal, parsed);
/// ```
///
/// [`http::Uri`]: crate::http::Uri
///
/// A location is deliberately *not* validated here. A `Location` field value is
/// a URI reference, which includes relative forms that only mean something
/// against the request URI, so rejecting anything at this point would refuse
/// values the specification permits.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Location(String);

impl Location {
    /// The location as it will appear in the header.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Takes the string out.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl From<String> for Location {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Location {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

// The conversion this type exists for. A typed URI is the sanctioned way to
// name another operation, so handing one to `Created::at` must cost nothing.
impl From<crate::http::Uri> for Location {
    fn from(value: crate::http::Uri) -> Self {
        Self(value.to_string())
    }
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

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
    pub location: Location,
}

impl<T> Created<T> {
    /// Creates a 201 response for a resource at `location`.
    ///
    /// Takes a string, or a route attribute's `relative_uri` directly.
    pub fn at(location: impl Into<Location>, body: T) -> Self {
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
    pub location: Location,
}

impl<const CODE: u16> Redirect<CODE> {
    /// Redirects to `location`.
    ///
    /// Takes a string, or a route attribute's `relative_uri` directly.
    pub fn to(location: impl Into<Location>) -> Self {
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

#[cfg(test)]
mod tests {
    /// A location arrives from three spellings, and the third is the reason the
    /// type exists.
    ///
    /// `relative_uri` returns an `http::Uri`, and neither that type nor `String`
    /// belongs to Kynos — so without a type of its own here, the sanctioned way to
    /// name another operation could not be handed to the two constructors that
    /// take a location.
    mod a_location_takes_every_spelling {
        use crate::response::status::{Created, Location, Redirect};

        #[test]
        fn the_three_spellings_agree() {
            let uri: crate::http::Uri = "/users/42".parse().expect("a valid reference");

            assert_eq!(Location::from("/users/42"), Location::from(uri));
            assert_eq!(
                Location::from("/users/42"),
                Location::from(String::from("/users/42"))
            );
        }

        #[test]
        fn a_typed_uri_reaches_a_created_response() {
            let uri: crate::http::Uri = "/users/42".parse().expect("a valid reference");
            let created = Created::at(uri, ());

            assert_eq!(created.location.as_str(), "/users/42");
        }

        #[test]
        fn a_typed_uri_reaches_a_redirect() {
            let uri: crate::http::Uri = "/users".parse().expect("a valid reference");
            let redirect = Redirect::<303>::to(uri);

            assert_eq!(redirect.location.as_str(), "/users");
        }

        /// A `Location` field value is a URI reference, which includes relative
        /// forms that only mean something against the request URI. Rejecting one
        /// here would refuse a value the specification permits.
        #[test]
        fn a_relative_reference_is_not_refused() {
            assert_eq!(Location::from("../sibling").as_str(), "../sibling");
        }
    }
}
