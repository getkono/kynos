//! Declaring response headers as part of the return type.

use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams},
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

/// The body's status is kept; the declared headers are written onto it.
///
/// Written as [`Continued::with_headers`](crate::middleware::Continued::with_headers)
/// writes them: a repeated `Set-Cookie` is appended, `Content-Encoding` replaced.
impl<T, H> IntoResponse for WithHeaders<T, H>
where
    T: IntoResponse,
    H: EncodeHeaders,
{
    fn into_response(self) -> Response {
        let mut response = self.body.into_response();
        crate::extract::params::header::write(response.headers_mut(), &self.headers);
        response
    }
}

/// The declared headers join every response the body describes.
///
/// A group whose [`DESCRIBED`](HeaderParams::DESCRIBED) is `false` joins none of
/// them, though it is still checked for conflicts.
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
mod tests;
