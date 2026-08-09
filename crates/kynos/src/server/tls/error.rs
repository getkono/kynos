//! What can go wrong configuring TLS.

/// A TLS certificate or verifier configuration failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TlsError {
    /// A PEM document was malformed or empty.
    #[error("invalid {kind} PEM: {message}")]
    Pem {
        /// The expected PEM material.
        kind: &'static str,
        /// Parser detail.
        message: String,
    },
    /// A private key did not match a supported signing algorithm.
    #[error("invalid TLS private key: {0}")]
    PrivateKey(String),
    /// An SNI server name was empty, invalid, or repeated.
    #[error("invalid SNI server name `{0}`")]
    ServerName(String),
    /// Client-certificate verification could not be configured.
    #[error("invalid client-certificate verifier: {0}")]
    ClientVerifier(String),
    /// A TLS duration was zero.
    #[error("TLS handshake timeout must be non-zero")]
    ZeroHandshakeTimeout,
}
