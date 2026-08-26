//! What a handler asks compression to do with one response.

use crate::{
    http,
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};

/// Whether one response may be encoded.
///
/// Negotiation decides *which* coding; this decides whether the question is
/// asked at all. It is a property of the response rather than of the route,
/// because the two reasons for overriding it are both per response: a body that
/// reflects a secret back to the client, and a body too large to be worth
/// sending as it is.
///
/// Reaches [`Compression`](super::Compression) through the response's
/// extensions, which is what lets a handler state it without any interceptor
/// having to declare a header for it. A response carrying none is
/// [`Automatic`](Encoding::Automatic).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Encoding {
    /// Negotiate, and encode if there is anything worth encoding to.
    #[default]
    Automatic,
    /// Never encode this response.
    ///
    /// The BREACH case. Compressing a body that mixes a secret with something
    /// the client chose leaks the secret through the length, and no
    /// negotiation makes that safe — so a response reflecting input beside a
    /// CSRF token, a session identifier or an API key says so here.
    ///
    /// RFC 9110 section 17.6 describes the attack and is deliberately not
    /// normative about it, so this is a policy the application owns rather than
    /// something the framework can infer.
    Disabled,
    /// Encode it, or refuse the request with 406.
    ///
    /// `required-compatible`: required, and compatible with whatever the client
    /// said it accepts. Identity stops being an acceptable answer, so a client
    /// that will take only identity is told 406 rather than handed forty
    /// megabytes uncompressed.
    ///
    /// This is the one setting that can turn a 200 into an error, and the error
    /// is one [`Compression`](super::Compression) already declares — mounting
    /// it contributes 406 to every covered operation whether or not any handler
    /// asks for this.
    ///
    /// Without a `Compression` covering the route it does nothing at all: an
    /// extension nobody reads is inert, and there is no interceptor to produce
    /// the refusal.
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
///     response::codec::json::Json,
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
/// Describes exactly what the response inside it describes. A compression
/// policy is not part of an operation's contract: it changes how the bytes
/// travel, not what they are, and the one status it can produce is declared by
/// the interceptor that produces it.
#[derive(Clone, Copy, Debug)]
pub struct WithEncoding<T> {
    /// The response.
    pub body: T,
    /// What compression may do with it.
    pub encoding: Encoding,
}

impl<T> WithEncoding<T> {
    /// Attaches `encoding` to `body`.
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
