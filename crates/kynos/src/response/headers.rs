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
    pub fn new(body: T, headers: H) -> Self {
        Self { body, headers }
    }
}

/// The body's status is kept: declared headers ride the response the body
/// already produces rather than making a different one.
///
/// Each encoded field is appended rather than inserted, so a group naming
/// `Set-Cookie` twice sends it twice instead of comma-joining two values that
/// may not be joined.
impl<T, H> IntoResponse for WithHeaders<T, H>
where
    T: IntoResponse,
    H: HeaderParams,
{
    fn into_response(self) -> Response {
        let mut response = self.body.into_response();
        let fields = response.headers_mut();

        for (name, value) in self.headers.encode() {
            fields.append(name, value);
        }

        // The same merge `Continued::with_headers` performs, so the two paths a
        // `HeaderParams` group can reach the wire by cannot disagree about what
        // a response varies on. A derived group varies on nothing, so this is a
        // no-op today and stays honest if one ever does.
        crate::middleware::vary_on(fields, H::VARIES);

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
