//! Responses whose status is fixed by their type.

use kynos_openapi::model::schema::types::SchemaType;

use crate::{
    http::{HeaderValue, Response, StatusCode, body::Body, header},
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};

/// Where a response points a client next.
///
/// The value of a `Location` header, converted from a string or from the
/// [`http::Uri`] a route attribute's `relative_uri` returns.
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
/// Not validated: a `Location` value is a URI reference, relative forms
/// included.
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
/// The `Location` header is required, so a 201 always says where.
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
    #[must_use]
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
    #[must_use]
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
    #[must_use]
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

/// The status a bare body type describes itself under.
const BODY_STATUS: u16 = 200;

/// The status [`Created`] fixes.
const CREATED: u16 = 201;

/// The status [`Accepted`] fixes.
const ACCEPTED: u16 = 202;

/// Sets `Location` on `response`, unless the value cannot be a field value.
///
/// Only a control character is refused, since it could forge the rest of the
/// message; the field is then omitted.
fn set_location(response: &mut Response, location: &Location) {
    if let Ok(value) = HeaderValue::from_str(location.as_str()) {
        response.headers_mut().insert(header::LOCATION, value);
    }
}

/// Describes a `Location` field that is always sent.
///
/// A plain string with no `format`, since relative references are legal.
fn location_header(description: &str) -> kynos_openapi::Header {
    kynos_openapi::Header::new(kynos_openapi::Schema::of_type(SchemaType::String))
        .with_description(description)
        .required(true)
}

/// Takes the response a body describes for itself, re-described for the status
/// its wrapper fixes.
///
/// In order of precedence:
///
/// * An entry the body already declares under the wrapper's status is returned
///   unchanged, or `None` if it is a `$ref`, which this may not overwrite.
/// * The body's 200 is moved to the wrapper's status with the wrapper's
///   description; a `$ref` 200 is left in place.
/// * With no 200, a body declaring exactly one response has its
///   representation carried over by [`sole_representation`], since that is
///   all it can send; otherwise the wrapper's response is empty.
///
/// A body's other statuses stay in the set, even though the wrapper re-keys
/// everything it sends.
fn body_response(
    description: &str,
    status: u16,
    body: &mut kynos_openapi::Responses,
) -> Option<kynos_openapi::Response> {
    // Read rather than remove, so the emitted order the body chose survives.
    match body
        .responses
        .get(&kynos_openapi::StatusPattern::Code(status).to_string())
    {
        Some(kynos_openapi::RefOr::Item(declared)) => return Some(declared.clone()),
        Some(kynos_openapi::RefOr::Ref(_)) => return None,
        None => {}
    }

    let key = kynos_openapi::StatusPattern::Code(BODY_STATUS).to_string();

    Some(match body.responses.shift_remove(&key) {
        Some(kynos_openapi::RefOr::Item(mut response)) => {
            response.description = Some(description.to_owned());
            response
        }
        Some(reference) => {
            body.responses.insert(key, reference);
            kynos_openapi::Response::new(description)
        }
        None => {
            let mut response = kynos_openapi::Response::new(description);
            if let Some(sole) = sole_representation(body) {
                // All three, as the re-key arm above carries.
                response.content.clone_from(&sole.content);
                response.headers.clone_from(&sole.headers);
                response.links.clone_from(&sole.links);
            }
            response
        }
    })
}

/// The representations a body declares, when it declares exactly one response
/// and that response carries any.
///
/// `None` for a second response even if it has no content, since it too is
/// sent under the wrapper's status, and for a `$ref`. A `default` counts as
/// the one response; a wildcard such as `4XX` does not.
fn sole_representation(body: &kynos_openapi::Responses) -> Option<&kynos_openapi::Response> {
    let mut keyed = body.responses.iter();

    let sole = match (body.default_response.as_ref(), keyed.next()) {
        (Some(default), None) => default,
        (None, Some((key, response))) => {
            key.parse::<u16>().ok()?;
            response
        }
        _ => return None,
    };

    if keyed.next().is_some() {
        return None;
    }

    let kynos_openapi::RefOr::Item(sole) = sole else {
        return None;
    };

    if sole.content.is_empty() {
        return None;
    }

    Some(sole)
}

/// What each redirect status tells a client, as RFC 9110 defines it.
///
/// Each states whether the move is permanent and whether the method survives.
fn redirect_description(code: u16) -> &'static str {
    match code {
        301 => "the resource has a new permanent URI, given by `Location`",
        302 => "the resource is temporarily at the URI given by `Location`",
        303 => "the response to this request is at the URI given by `Location`, retrieved with GET",
        307 => "the resource is temporarily at `Location`; the method is preserved on replay",
        308 => "the resource has a new permanent URI in `Location`; the method survives replay",
        // Unreachable while `ValidRedirectCode` witnesses only the five above.
        _ => "the client is directed to the URI given by `Location`",
    }
}

impl IntoResponse for NoContent {
    fn into_response(self) -> Response {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::NO_CONTENT;
        response
    }
}

impl Responses for NoContent {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        kynos_openapi::Responses::new().with(
            204,
            kynos_openapi::Response::new("the request succeeded and there is no content to send"),
        )
    }
}

impl<T: IntoResponse> IntoResponse for Created<T> {
    fn into_response(self) -> Response {
        let mut response = self.body.into_response();
        *response.status_mut() = StatusCode::CREATED;
        set_location(&mut response, &self.location);
        response
    }
}

impl<T: Responses> Responses for Created<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let mut responses = T::responses(registry);
        let Some(created) = body_response("the resource was created", CREATED, &mut responses)
        else {
            return responses;
        };

        responses.with(
            CREATED,
            created.with_header(
                "Location",
                location_header("Where the created resource lives"),
            ),
        )
    }
}

impl<T: IntoResponse> IntoResponse for Accepted<T> {
    fn into_response(self) -> Response {
        let mut response = self.body.into_response();
        *response.status_mut() = StatusCode::ACCEPTED;
        response
    }
}

impl<T: Responses> Responses for Accepted<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let mut responses = T::responses(registry);
        let Some(accepted) = body_response(
            "the request was accepted, and the processing it asked for has not completed",
            ACCEPTED,
            &mut responses,
        ) else {
            return responses;
        };

        responses.with(ACCEPTED, accepted)
    }
}

impl<const CODE: u16> IntoResponse for Redirect<CODE>
where
    (): ValidRedirectCode<CODE>,
{
    fn into_response(self) -> Response {
        let mut response = Response::new(Body::empty());
        // The witness admits only valid status codes.
        *response.status_mut() =
            StatusCode::from_u16(CODE).expect("a witnessed redirect code is a status code");
        set_location(&mut response, &self.location);
        response
    }
}

impl<const CODE: u16> Responses for Redirect<CODE>
where
    (): ValidRedirectCode<CODE>,
{
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        kynos_openapi::Responses::new().with(
            CODE,
            kynos_openapi::Response::new(redirect_description(CODE))
                .with_header("Location", location_header("Where to go instead")),
        )
    }
}

#[cfg(test)]
mod tests;
