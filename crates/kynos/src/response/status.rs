//! Responses whose status is fixed by their type.

use std::{borrow::Cow, str::FromStr};

use bytes::Bytes;
use kynos_openapi::model::schema::types::SchemaType;

use crate::{
    http::{HeaderValue, Response, StatusCode, body::bodiless, header},
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};

/// Where a response points a client next.
///
/// The value of a `Location` header: the [`http::Uri`] a route attribute's
/// `relative_uri` or a [`Link`]'s `uri` returns, a literal through
/// [`from_static`](Location::from_static), or any other string through
/// [`TryFrom`] or [`str::parse`].
///
/// ```
/// use kynos::response::status::Location;
///
/// let literal = Location::from_static("/users/42");
/// let owned = Location::try_from(format!("/users/{}", 42)).unwrap();
/// let parsed = Location::from("/users/42".parse::<kynos::http::Uri>().unwrap());
/// assert_eq!(literal, owned);
/// assert_eq!(literal, parsed);
/// assert!("/users\n42".parse::<Location>().is_err());
/// ```
///
/// [`http::Uri`]: crate::http::Uri
/// [`Link`]: crate::extract::link::Link
///
/// Always an RFC 9110 field value, since [`Created`] and [`Redirect`] declare
/// the header required and so must always send it. Nothing beyond that is
/// checked: a `Location` value is a URI reference, relative forms included.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Location(Cow<'static, str>);

/// Why a string cannot be a [`Location`]: it is not an RFC 9110 field value
/// (§5.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidLocation {
    /// A control character other than a horizontal tab, or DEL, which could
    /// end the field and forge the rest of the message.
    #[error("a `Location` holds no control character but a tab")]
    Control,
    /// A space or tab at either end, which a recipient strips.
    #[error("a `Location` neither starts nor ends with whitespace")]
    Whitespace,
}

impl Location {
    /// A location from a literal.
    ///
    /// In a `const` item, a value that is not a field value fails to compile.
    ///
    /// ```
    /// use kynos::response::status::Location;
    ///
    /// const USERS: Location = Location::from_static("/users");
    /// assert_eq!(USERS.as_str(), "/users");
    /// ```
    ///
    /// ```compile_fail
    /// use kynos::response::status::Location;
    ///
    /// const FORGED: Location = Location::from_static("/users\r\nSet-Cookie: a=b");
    /// ```
    ///
    /// # Panics
    ///
    /// If `value` is not a field value, as [`InvalidLocation`] describes.
    #[must_use]
    pub const fn from_static(value: &'static str) -> Self {
        assert!(
            validate(value).is_ok(),
            "a `Location` is a field value: no control character, no surrounding space"
        );
        Self(Cow::Borrowed(value))
    }

    /// The location as it will appear in the header.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Takes the string out.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0.into_owned()
    }

    /// The header value, without copying.
    fn into_header_value(self) -> HeaderValue {
        let bytes = match self.0 {
            Cow::Borrowed(value) => Bytes::from_static(value.as_bytes()),
            Cow::Owned(value) => Bytes::from(value),
        };
        // Construction checked what `HeaderValue` checks, and more.
        HeaderValue::from_maybe_shared(bytes).expect("a `Location` is a field value")
    }
}

/// Whether `value` is an RFC 9110 field value (§5.5).
///
/// `obs-text` is admitted, as [`HeaderValue`] admits it.
const fn validate(value: &str) -> Result<(), InvalidLocation> {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if (byte < b' ' && byte != b'\t') || byte == 0x7f {
            return Err(InvalidLocation::Control);
        }
        index += 1;
    }
    if let [b' ' | b'\t', ..] | [.., b' ' | b'\t'] = bytes {
        return Err(InvalidLocation::Whitespace);
    }
    Ok(())
}

impl TryFrom<String> for Location {
    type Error = InvalidLocation;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate(&value)?;
        Ok(Self(Cow::Owned(value)))
    }
}

impl TryFrom<&str> for Location {
    type Error = InvalidLocation;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        validate(value)?;
        Ok(Self(Cow::Owned(value.to_owned())))
    }
}

impl FromStr for Location {
    type Err = InvalidLocation;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_from(value)
    }
}

/// Infallible: a URI holds no control character and no whitespace.
impl From<crate::http::Uri> for Location {
    fn from(value: crate::http::Uri) -> Self {
        Self(Cow::Owned(value.to_string()))
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
/// The `Location` header is required, so a 201 always says where. A body
/// answering 4xx or 5xx, such as a `Json` that failed to serialize, is sent
/// unchanged, with neither 201 nor `Location`.
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
    /// Takes a [`Location`], a route attribute's `relative_uri`, or a `Link`'s
    /// `uri`.
    #[must_use]
    pub fn at(location: impl Into<Location>, body: T) -> Self {
        Self {
            body,
            location: location.into(),
        }
    }
}

/// A 202 Accepted response for work that has not finished.
///
/// A body that answers with a 4xx or 5xx is sent unchanged rather than as 202.
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
/// compile. That rules out a redirect answered with a code that is not one,
/// such as 304, which carries no `Location` to follow. Choosing among the five
/// stays the caller's: 302 where 307 was meant still compiles.
///
/// ```compile_fail
/// fn response<T: kynos::response::IntoResponse>(value: T) { drop(value); }
/// use kynos::response::status::{Location, Redirect};
/// response(Redirect::<304>::to(Location::from_static("/cached")));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Redirect<const CODE: u16> {
    /// The target of the redirect.
    pub location: Location,
}

impl<const CODE: u16> Redirect<CODE> {
    /// Redirects to `location`.
    ///
    /// Takes a [`Location`], a route attribute's `relative_uri`, or a `Link`'s
    /// `uri`.
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

/// Sets `Location` on `response`; a [`Location`] is always a field value.
fn set_location(response: &mut Response, location: Location) {
    response
        .headers_mut()
        .insert(header::LOCATION, location.into_header_value());
}

/// Whether the body answered with an error, which a success wrapper sends as
/// is: a problem's `status` must match the one sent (RFC 9457 §3.1.2).
pub(crate) fn is_failure(response: &Response) -> bool {
    response.status().is_client_error() || response.status().is_server_error()
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
/// * With no 200, the one response a body sends under the wrapper's status
///   has its representation carried over by [`sole_representation`];
///   otherwise the wrapper's response is empty.
///
/// A body's other statuses stay in the set; its 4xx and 5xx go out unchanged.
pub(crate) fn body_response(
    description: &str,
    status: u16,
    body: &mut kynos_openapi::Responses,
) -> Option<kynos_openapi::Response> {
    // Read rather than remove, so the emitted order the body chose survives.
    match body.get(status) {
        Some(kynos_openapi::RefOr::Item(declared)) => return Some(declared.clone()),
        Some(kynos_openapi::RefOr::Ref(_)) => return None,
        None => {}
    }

    let key = kynos_openapi::StatusPattern::Code(BODY_STATUS).to_string();

    // Read before removing, so a `$ref` 200 is never taken out of its place.
    if body
        .responses
        .get(&key)
        .is_some_and(kynos_openapi::RefOr::is_ref)
    {
        return Some(kynos_openapi::Response::new(description));
    }

    if let Some(kynos_openapi::RefOr::Item(mut response)) = body.responses.shift_remove(&key) {
        response.description = Some(description.to_owned());
        return Some(response);
    }

    let mut response = kynos_openapi::Response::new(description);
    if let Some(sole) = sole_representation(body) {
        // All three, as the re-key above carries.
        response.content.clone_from(&sole.content);
        response.headers.clone_from(&sole.headers);
        response.links.clone_from(&sole.links);
    }
    Some(response)
}

/// The representations of the one response a body declares outside 4xx and
/// 5xx, which keep their own status, if it carries any.
///
/// `None` for a second such response even if bodiless, and for a `$ref`. A
/// `default` counts as the one response; a `2XX` wildcard does not.
fn sole_representation(body: &kynos_openapi::Responses) -> Option<&kynos_openapi::Response> {
    let mut keyed = body
        .responses
        .iter()
        .filter(|(key, _)| !key.starts_with(['4', '5']));

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

    (!sole.content.is_empty()).then_some(sole)
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
        bodiless(StatusCode::NO_CONTENT)
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
        if is_failure(&response) {
            return response;
        }
        *response.status_mut() = StatusCode::CREATED;
        set_location(&mut response, self.location);
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
        if is_failure(&response) {
            return response;
        }
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
        // The witness admits only valid status codes.
        let mut response = bodiless(
            StatusCode::from_u16(CODE).expect("a witnessed redirect code is a status code"),
        );
        set_location(&mut response, self.location);
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
