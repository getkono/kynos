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
    H: EncodeHeaders,
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
mod tests;
