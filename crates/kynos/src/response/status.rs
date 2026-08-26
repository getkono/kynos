//! Responses whose status is fixed by their type.

use kynos_openapi::model::schema::types::SchemaType;

use crate::{
    http::{HeaderValue, Response, StatusCode, body::Body, header},
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

/// The status a bare body type describes itself under.
///
/// Changing it is what every wrapper in this module is for.
const BODY_STATUS: u16 = 200;

/// Sets `Location` on `response`, unless the value cannot be a field value.
///
/// The only strings refused here are ones holding a control character, which a
/// URI reference never legitimately does and which a field value must never
/// carry: writing one out would let a caller-supplied string forge the rest of
/// the message. Omitting the field is the safe half of that trade, and
/// [`Location`] accepts everything else the specification permits.
fn set_location(response: &mut Response, location: &Location) {
    if let Ok(value) = HeaderValue::from_str(location.as_str()) {
        response.headers_mut().insert(header::LOCATION, value);
    }
}

/// Describes a `Location` field that is always sent.
///
/// A string rather than a schema with a `format`, because the value is a URI
/// *reference* and the relative forms are as legal as the absolute ones.
fn location_header(description: &str) -> kynos_openapi::Header {
    kynos_openapi::Header::new(kynos_openapi::Schema::of_type(SchemaType::String))
        .with_description(description)
        .required(true)
}

/// Takes the response a body describes for itself, re-described for the status
/// its wrapper fixes.
///
/// A bare body type describes itself under 200, so the wrapper's own status is
/// where that response belongs: the content, headers and links carry over, and
/// the description becomes the wrapper's, since what a 201 or a 202 means is
/// the half of the statement a body was never in a position to make. Anything
/// the body declared under another status stays where it is — that was not
/// 200's to re-key — and a body describing no 200 at all leaves the wrapper
/// describing an empty response, which is what it then produces.
///
/// A `$ref` is left in place rather than re-described, because it names a
/// response the document holds elsewhere and every other use of it would be
/// re-described too.
fn body_response(
    description: &str,
    body: &mut kynos_openapi::Responses,
) -> kynos_openapi::Response {
    let key = kynos_openapi::StatusPattern::Code(BODY_STATUS).to_string();

    match body.responses.shift_remove(&key) {
        Some(kynos_openapi::RefOr::Item(mut response)) => {
            description.clone_into(&mut response.description);
            response
        }
        Some(reference) => {
            body.responses.insert(key, reference);
            kynos_openapi::Response::new(description)
        }
        None => kynos_openapi::Response::new(description),
    }
}

/// What each redirect status tells a client, as RFC 9110 defines it.
///
/// The five differ in two ways a consumer acts on — whether the move is
/// permanent, and whether the method survives the replay — so one description
/// for all of them would leave out the whole of the choice.
fn redirect_description(code: u16) -> &'static str {
    match code {
        301 => "the resource has a new permanent URI, given by `Location`",
        302 => "the resource is temporarily at the URI given by `Location`",
        303 => "the response to this request is at the URI given by `Location`, retrieved with GET",
        307 => "the resource is temporarily at `Location`; the method is preserved on replay",
        308 => "the resource has a new permanent URI in `Location`; the method survives replay",
        // Unreachable while `ValidRedirectCode` witnesses exactly the five
        // statuses above, which is what bounds every caller.
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
        let created = body_response("the resource was created", &mut responses).with_header(
            "Location",
            location_header("Where the created resource lives"),
        );

        responses.with(201, created)
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
        let accepted = body_response(
            "the request was accepted, and the processing it asked for has not completed",
            &mut responses,
        );

        responses.with(202, accepted)
    }
}

impl<const CODE: u16> IntoResponse for Redirect<CODE>
where
    (): ValidRedirectCode<CODE>,
{
    fn into_response(self) -> Response {
        let mut response = Response::new(Body::empty());
        // The witness admits five statuses and every one of them is a status
        // code, so the conversion cannot fail for a `Redirect` that exists.
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

    /// Every status a wrapper fixes, produced and declared.
    ///
    /// The two halves at once, because a wrapper's whole purpose is that they
    /// cannot come apart: a `Created<T>` that sent 201 and declared 200 would
    /// be a response no consumer could handle, and neither half alone would
    /// notice.
    mod a_wrapper_produces_and_declares_one_status {
        use crate::{
            extract::body::text::Text,
            http::{StatusCode, header},
            response::{
                IntoResponse, Responses,
                status::{Accepted, Created, NoContent, Redirect},
            },
            schema::registry::Registry,
        };

        /// The statuses a type declares, as the keys a consumer would read.
        fn declared<T: Responses>() -> Vec<String> {
            let mut registry = Registry::new();
            T::responses(&mut registry)
                .responses
                .keys()
                .cloned()
                .collect()
        }

        #[test]
        fn no_content_sends_204_and_no_body() {
            let response = NoContent.into_response();

            assert_eq!(response.status(), StatusCode::NO_CONTENT);
            assert_eq!(declared::<NoContent>(), ["204"]);
        }

        #[test]
        fn created_sends_201_and_says_where() {
            let response = Created::at("/users/42", Text("body".to_owned())).into_response();

            assert_eq!(response.status(), StatusCode::CREATED);
            assert_eq!(
                response
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|value| value.to_str().ok()),
                Some("/users/42")
            );

            // The wrapped body's own 200 is gone: a `Created<T>` sends 201, and
            // leaving the 200 behind would declare a response the type cannot
            // produce.
            assert_eq!(declared::<Created<Text>>(), ["201"]);
        }

        #[test]
        fn accepted_sends_202_and_keeps_the_body() {
            let response = Accepted::new(Text("body".to_owned())).into_response();

            assert_eq!(response.status(), StatusCode::ACCEPTED);
            // No `Location`: 202 says the work has not finished, so there is
            // nothing yet to point at.
            assert!(response.headers().get(header::LOCATION).is_none());
            assert_eq!(declared::<Accepted<Text>>(), ["202"]);
        }

        /// Every code the witness admits, over the whole set.
        ///
        /// A closed enumeration, so all five rather than a sample: the ones a
        /// sample would skip are 301 and 308, which differ from the other three
        /// in whether a client may rewrite the method — the one thing a caller
        /// picks a redirect code for.
        #[test]
        fn every_witnessed_redirect_code_is_the_one_it_sends() {
            fn case<const CODE: u16>()
            where
                (): crate::response::status::ValidRedirectCode<CODE>,
            {
                let response = Redirect::<CODE>::to("/elsewhere").into_response();

                assert_eq!(response.status().as_u16(), CODE);
                assert_eq!(
                    response
                        .headers()
                        .get(header::LOCATION)
                        .and_then(|value| value.to_str().ok()),
                    Some("/elsewhere")
                );
                assert_eq!(declared::<Redirect<CODE>>(), [CODE.to_string()]);
            }

            case::<301>();
            case::<302>();
            case::<303>();
            case::<307>();
            case::<308>();
        }

        /// The witness, counted against the cases above.
        #[test]
        fn every_witnessed_redirect_code_has_a_case() {
            const SOURCE: &str = include_str!("status.rs");
            /// 301, 302, 303, 307 and 308.
            const WITNESSED: usize = 5;
            // Spelled in two pieces: `SOURCE` is this file.
            const NEEDLE: &str = concat!("impl ValidRedirect", "Code<");

            let declared = SOURCE.matches(NEEDLE).count();

            assert_eq!(
                declared, WITNESSED,
                "`status.rs` witnesses {declared} redirect code(s) and {WITNESSED} have a case"
            );
        }

        /// A redirect declares `Location` as a header a client must be able to
        /// read, because a redirect without one names nowhere to go.
        #[test]
        fn a_redirect_declares_the_location_it_sends() {
            let mut registry = Registry::new();
            let responses = <Redirect<303> as Responses>::responses(&mut registry);

            assert!(
                responses.responses["303"]
                    .as_item()
                    .expect("an inline response")
                    .headers
                    .contains_key("Location")
            );
        }
    }
}
