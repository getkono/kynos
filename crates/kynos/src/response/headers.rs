//! Declaring response headers as part of the return type.

use crate::{
    extract::params::header::HeaderParams,
    http::Response,
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};

/// A response carrying declared headers alongside its body.
///
/// `H` derives `HeaderParams`, so each header appears in `Response.headers` with
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
    #[must_use]
    pub fn new(body: T, headers: H) -> Self {
        Self { body, headers }
    }
}

/// The body's status is kept: declared headers ride the response the body
/// already produces rather than making a different one.
///
/// Through [`header::write`](crate::extract::params::header), which is also what
/// [`Continued::with_headers`](crate::middleware::Continued::with_headers)
/// calls — so a group naming `Set-Cookie` twice sends it twice on either path,
/// and one naming `Content-Encoding` replaces on either. This comment used to
/// say the two could not disagree while they were two functions that did.
impl<T, H> IntoResponse for WithHeaders<T, H>
where
    T: IntoResponse,
    H: HeaderParams,
{
    fn into_response(self) -> Response {
        let mut response = self.body.into_response();
        crate::extract::params::header::write(response.headers_mut(), &self.headers);
        response
    }
}

/// The declared headers join every response the body describes, since every one
/// of them is produced through this wrapper and carries them.
///
/// A group whose [`DESCRIBED`](HeaderParams::DESCRIBED) is `false` joins none of
/// them: it is still checked for conflicts, it is simply not worth telling a
/// consumer about.
impl<T, H> Responses for WithHeaders<T, H>
where
    T: Responses,
    H: HeaderParams,
{
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let mut responses = T::responses(registry);

        if !H::DESCRIBED {
            return responses;
        }

        let declared = H::response_headers(registry);
        let described = responses
            .default_response
            .iter_mut()
            .chain(responses.responses.values_mut());

        for response in described {
            // A `$ref` names a response the document holds elsewhere, and
            // declaring a field on it would declare it on every other use.
            if let kynos_openapi::RefOr::Item(response) = response {
                for (name, header) in &declared {
                    response.headers.insert(name.clone(), header.clone());
                }
            }
        }

        responses
    }
}

#[cfg(test)]
mod tests {
    use super::WithHeaders;
    use crate::{
        extract::params::header::HeaderParams,
        http::{HeaderName, HeaderValue, Response, header},
        middleware::Continued,
        response::IntoResponse,
    };

    /// A group naming one field twice.
    #[derive(Clone, Copy)]
    struct TwoCookies;

    impl HeaderParams for TwoCookies {
        const NAMES: &'static [&'static str] = &["set-cookie"];
        const REPEATABLE: bool = true;

        fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
            vec![
                (header::SET_COOKIE, HeaderValue::from_static("first=1")),
                (header::SET_COOKIE, HeaderValue::from_static("second=2")),
            ]
        }
    }

    /// A group naming one field once.
    #[derive(Clone, Copy)]
    struct OneEncoding;

    impl HeaderParams for OneEncoding {
        const NAMES: &'static [&'static str] = &["content-encoding"];
        const VARIES: &'static [&'static str] = &["accept-encoding"];

        fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
            vec![(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"))]
        }
    }

    /// What one group writes, through both ways a group reaches the wire.
    fn both_paths<G: HeaderParams + Copy>(
        group: G,
        name: &HeaderName,
    ) -> (Vec<String>, Vec<String>) {
        let read = |response: Response| {
            response
                .headers()
                .get_all(name)
                .iter()
                .map(|value| value.to_str().expect("a printable field").to_owned())
                .collect::<Vec<_>>()
        };

        let handler = read(WithHeaders::new((), group).into_response());
        let interceptor = read(
            Continued::new(Response::new(crate::http::body::Body::empty()))
                .with_headers(group)
                .into_response(),
        );

        (handler, interceptor)
    }

    /// The invariant the two paths were claimed to hold and did not.
    ///
    /// Asserting they *agree* rather than asserting each separately: two tests
    /// that happen to expect the same thing is what the code was, and it is
    /// exactly what stopped anyone noticing. One of these appended and the
    /// other inserted, so a group naming `Set-Cookie` twice reached the wire
    /// whole from a handler and truncated from an interceptor.
    #[test]
    fn a_group_writes_the_same_fields_whichever_path_it_reaches_the_wire_by() {
        let (handler, interceptor) = both_paths(TwoCookies, &header::SET_COOKIE);
        assert_eq!(handler, interceptor);
        assert_eq!(handler, ["first=1", "second=2"]);

        let (handler, interceptor) = both_paths(OneEncoding, &header::CONTENT_ENCODING);
        assert_eq!(handler, interceptor);
        assert_eq!(handler, ["gzip"]);
    }

    /// `Vary` is merged on both paths too, which is the half that was already
    /// shared and has to stay so.
    #[test]
    fn a_group_varies_the_same_whichever_path_it_reaches_the_wire_by() {
        let (handler, interceptor) = both_paths(OneEncoding, &header::VARY);
        assert_eq!(handler, interceptor);
        assert_eq!(handler, ["accept-encoding"]);
    }
}
