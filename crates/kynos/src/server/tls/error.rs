//! What can go wrong configuring TLS.

/// A boxed cause, so that a rustls or webpki failure stays walkable without its
/// type reaching this crate's public API.
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
    /// Unlike [`Pem`](Self::Pem), the document was well-formed and empty.
    #[error("no {kind} found in the PEM document")]
    EmptyPem {
        /// The expected PEM material.
        kind: &'static str,
    },
    /// A private key did not match a supported signing algorithm.
    #[error("invalid TLS private key")]
    PrivateKey(#[source] Cause),
    /// An SNI server name was empty, invalid, or repeated.
    #[error("invalid SNI server name `{0}`")]
    ServerName(String),
    /// Client-certificate verification could not be configured.
    #[error("invalid client-certificate verifier")]
    ClientVerifier(#[source] Cause),
    /// The crypto provider could not serve the TLS versions Kynos enables.
    ///
    /// Reachable only through a provider a caller installed as the process
    /// default: a suite list covering neither TLS 1.2 nor 1.3, or key-exchange
    /// groups none of those suites can use.
    #[error("the TLS crypto provider serves none of the enabled protocol versions")]
    CryptoProvider(#[source] Cause),
    /// The session-ticket keys could not be generated.
    ///
    /// Reached when the system's random-number source fails.
    #[error("could not generate TLS session-ticket keys")]
    Ticketer(#[source] Cause),
    /// The crypto provider cannot seal session tickets under shared keys.
    ///
    /// Reachable only through a provider a caller installed as the process
    /// default that lacks `TLS13_AES_256_GCM_SHA384` or its QUIC support.
    #[error("the TLS crypto provider offers no AES-256-GCM to seal shared session tickets with")]
    TicketCipher,
    /// A shared session-ticket lifetime was under a second or over seven days.
    ///
    /// Seven days is the most RFC 8446 §4.6.1 lets a server advertise.
    #[error("TLS session-ticket lifetime must be between one second and seven days, not {0:?}")]
    TicketLifetime(std::time::Duration),
    /// A TLS duration was zero.
    #[error("TLS handshake timeout must be non-zero")]
    ZeroHandshakeTimeout,
}
