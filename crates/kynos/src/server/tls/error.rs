//! What can go wrong configuring TLS.

/// A boxed cause, so that a rustls or webpki failure survives without its type
/// reaching this crate's public API.
///
/// Naming the concrete types here would put rustls's semantic version inside
/// Kynos's, and would name rustls outside `server/tls/` the moment a caller
/// matched on one. Boxing keeps the cause walkable and the containment rule in
/// [`nfr.md`](https://github.com/getkono/kynos/blob/master/docs/nfr.md#dependencies) intact.
type Cause = Box<dyn std::error::Error + Send + Sync>;

/// A TLS certificate or verifier configuration failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TlsError {
    /// A PEM document was malformed.
    #[error("invalid {kind} PEM")]
    Pem {
        /// The expected PEM material.
        kind: &'static str,
        /// The parser failure.
        #[source]
        source: Cause,
    },
    /// A PEM document held none of the material it was read for.
    ///
    /// Separate from [`Pem`](Self::Pem) because nothing failed to parse: the
    /// document was well-formed and empty, which is a different mistake and has
    /// no cause to carry.
    #[error("no {kind} found in the PEM document")]
    EmptyPem {
        /// The expected PEM material.
        kind: &'static str,
    },
    /// A private key did not match a supported signing algorithm.
    #[error("invalid TLS private key")]
    PrivateKey(#[source] Cause),
    /// An SNI server name was empty, invalid, or repeated.
    ///
    /// The name is the whole of what went wrong, so this carries no cause: two
    /// of the three ways to reach it — an empty name and a repeated one — are
    /// found by Kynos rather than reported by rustls.
    #[error("invalid SNI server name `{0}`")]
    ServerName(String),
    /// Client-certificate verification could not be configured.
    #[error("invalid client-certificate verifier")]
    ClientVerifier(#[source] Cause),
    /// A TLS duration was zero.
    #[error("TLS handshake timeout must be non-zero")]
    ZeroHandshakeTimeout,
}
