//! What a handler asks compression to do with one response.

use crate::{
    http,
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};

/// Whether one response may be encoded.
///
/// Negotiation decides *which* coding; this decides whether the question is
/// asked at all, per response.
///
/// Reaches [`Compression`](super::Compression) through the response's
/// extensions (see [`WithEncoding`]). A response carrying none is
/// [`Automatic`](Encoding::Automatic).
///
/// `#[non_exhaustive]`: Kynos may add a policy without a breaking change.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Encoding {
    /// Negotiate, and encode if there is anything worth encoding to.
    #[default]
    Automatic,
    /// Never encode this response.
    ///
    /// The BREACH case (RFC 9110 section 17.6): compressing a body that mixes a
    /// secret with something the client chose leaks the secret through the
    /// length. Use it for a response reflecting input beside a CSRF token, a
    /// session identifier or an API key.
    Disabled,
    /// Encode it, or refuse the request with 406.
    ///
    /// Identity stops being an acceptable answer, so a client that will take
    /// only identity is told 406 rather than handed the body uncompressed.
    ///
    /// It also outranks [`min_size`](super::Compression::min_size). An empty
    /// body, or one [`Compression`](super::Compression) must leave alone (a
    /// ranged or strongly tagged response), is refused with 406 whatever the
    /// client accepts. The 406 is one `Compression` already declares.
    ///
    /// Without a `Compression` covering the route it does nothing at all.
    Required,
}

impl Encoding {
    /// The policy `extensions` states, or the default.
    pub(crate) fn of_extensions(extensions: &http::Extensions) -> Self {
        extensions.get().copied().unwrap_or_default()
    }
}

/// A response carrying a compression policy.
///
/// ```no_run
/// # #[cfg(all(feature = "compression", feature = "json"))]
/// # {
/// use kynos::{
///     middleware::compression::policy::{Encoding, WithEncoding},
///     extract::body::json::Json,
/// };
/// # #[derive(kynos::Schema, serde::Serialize)]
/// # struct Receipt { token: String }
/// # fn receipt() -> Receipt { todo!() }
///
/// // Echoes a token back beside attacker-chosen input, so it is never encoded.
/// fn confirm() -> WithEncoding<Json<Receipt>> {
///     WithEncoding::new(Json(receipt()), Encoding::Disabled)
/// }
/// # }
/// ```
///
/// Describes exactly what the response inside it describes: the one status a
/// policy can produce is declared by the interceptor that produces it.
#[derive(Clone, Copy, Debug)]
pub struct WithEncoding<T> {
    /// The response.
    pub body: T,
    /// What compression may do with it.
    pub encoding: Encoding,
}

impl<T> WithEncoding<T> {
    /// Attaches `encoding` to `body`.
    #[must_use]
    pub fn new(body: T, encoding: Encoding) -> Self {
        Self { body, encoding }
    }
}

impl<T: IntoResponse> IntoResponse for WithEncoding<T> {
    fn into_response(self) -> http::Response {
        let mut response = self.body.into_response();
        response.extensions_mut().insert(self.encoding);
        response
    }
}

impl<T: Responses> Responses for WithEncoding<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        T::responses(registry)
    }
}
