//! Serving every listener over TLS.

mod certificate;
pub(in crate::server) mod document;

pub mod error;
pub mod ticket;

use std::{collections::BTreeMap, fmt, num::NonZeroUsize, sync::Arc, time::Duration};

use tokio_rustls::rustls::{
    RootCertStore, ServerConfig as RustlsServerConfig,
    crypto::{CryptoProvider, aws_lc_rs::Ticketer},
    pki_types::{CertificateDer, CertificateRevocationListDer, pem::PemObject},
    server::{NoServerSessionStorage, ServerSessionMemoryCache, WebPkiClientVerifier},
};

use crate::server::tls::{
    certificate::{
        CertificateMaterial, StaticCertificateResolver, certified_key, parse_certificate_material,
        parse_certificates,
    },
    error::TlsError,
    ticket::{SharedTicketer, TicketKeys},
};

/// Mandatory client-certificate verification material.
#[derive(Clone, Debug)]
pub struct ClientCertificateConfig {
    roots: Vec<CertificateDer<'static>>,
    crls: Vec<CertificateRevocationListDer<'static>>,
}

impl ClientCertificateConfig {
    /// Parses PEM trust anchors used to verify client certificates.
    pub fn from_pem_roots(roots: &[u8]) -> std::result::Result<Self, TlsError> {
        Ok(Self {
            roots: parse_certificates(roots, "client root certificate")?,
            crls: Vec::new(),
        })
    }

    /// Adds PEM certificate-revocation lists.
    pub fn with_pem_crls(mut self, crls: &[u8]) -> std::result::Result<Self, TlsError> {
        let parsed = CertificateRevocationListDer::pem_slice_iter(crls)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|error| TlsError::Pem {
                kind: "certificate revocation list",
                source: Box::new(error),
            })?;
        if parsed.is_empty() {
            return Err(TlsError::EmptyPem {
                kind: "certificate revocation list",
            });
        }
        self.crls.extend(parsed);
        Ok(self)
    }
}

/// How a returning client resumes its session instead of paying a full
/// handshake.
///
/// A full TLS handshake costs the server an asymmetric signature and a key
/// exchange; a resumed one costs neither. For a service whose clients reconnect
/// often, that is most of its TLS work.
///
/// Every listener of a server shares one session store, so a
/// session established without a client certificate can never be resumed where
/// one is required: [`require_client_certificate`](TlsConfig::require_client_certificate)
/// applies to the whole server, and a resumed session carries the certificate
/// its full handshake verified.
///
/// That certificate is not verified again on resumption, and each resumption
/// issues fresh tickets carrying it, so a client that keeps reconnecting before
/// its ticket lapses — within six hours is always soon enough — keeps the
/// identity past its certificate's expiry for the life of the process. A
/// deployment that must re-verify every connection against
/// the certificate's validity period uses [`Disabled`](Self::Disabled).
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub enum SessionResumption {
    /// Stateless tickets: the session is sealed into a ticket the client holds,
    /// so resuming depends on nothing the server stored and no cache bounds how
    /// many clients can resume.
    ///
    /// The ticket keys are random, and a key stays accepted for one rotation
    /// after it stops issuing. Rotation happens on the first handshake more than
    /// six hours after the last one, so a ticket is honoured for at least six
    /// hours, for about twelve on a server with steady traffic, and for longer
    /// on one that goes quiet. The keys live only in this process. Replicas behind a load
    /// balancer therefore cannot resume one another's sessions, and neither can
    /// a restarted process; [`SharedTickets`](Self::SharedTickets) is the
    /// variant under which they can. Tickets use RFC 5077 §4's construction, with
    /// AES-256 and HMAC-SHA256, sealed by `aws-lc-rs` even when a caller installed
    /// another provider as the process default, because rustls's provider
    /// interface carries no ticketer.
    ///
    /// A TLS 1.2 ticket carries the session's master secret, so anyone who later
    /// obtains a ticket key — held in memory until two rotations have passed,
    /// about twelve hours under steady traffic — can
    /// decrypt the TLS 1.2 sessions recorded under it. TLS 1.3 resumption
    /// always runs a fresh key exchange, so its sessions keep forward secrecy.
    ///
    /// A ticket key also vouches for identity: a resumed session takes its
    /// client certificate chain from the ticket, so anyone holding a key can
    /// mint a ticket naming any chain and resume as any mutual-TLS identity,
    /// over TLS 1.2 or 1.3, for as long as that key is accepted. A mutual-TLS
    /// deployment that cannot accept that uses [`Disabled`](Self::Disabled) or
    /// [`Cache`](Self::Cache).
    ///
    /// rustls's in-memory cache of 256 sessions stays beside the tickets: a
    /// TLS 1.2 full handshake that presented no ticket still writes one entry
    /// to it, and a TLS 1.2 client that takes no tickets resumes from it.
    #[default]
    Tickets,
    /// A server-side cache of at most `capacity` entries, and no stateless
    /// tickets.
    ///
    /// Each entry costs server memory, and a service with more
    /// recently-connected clients than the cache holds evicts sessions, so
    /// those clients pay a full handshake. A TLS 1.2 session is one entry; a
    /// TLS 1.3 handshake stores two, each resumed once, so the cache holds
    /// about half as many TLS 1.3 clients as it has entries. Nothing leaves
    /// the process.
    Cache {
        /// The most entries the cache holds before evicting; rustls may round
        /// it up.
        ///
        /// The cache's table is reserved for that many up front when the
        /// server is [prepared](crate::server::Server::prepare), not grown as
        /// entries arrive.
        capacity: NonZeroUsize,
    },
    /// Stateless tickets under keys the operator supplies, so that every
    /// replica given the same keys resumes the sessions the others issued, and
    /// a restarted process resumes its own.
    ///
    /// What [`Tickets`](Self::Tickets) says of a ticket key holds here, with
    /// one difference that changes its weight: the key no longer dies with a
    /// process, and Kynos rotates nothing. What a key exposes and how to
    /// rotate one are [`TicketKeys`]'s to state; the construction and the
    /// provider that performs it are [`TicketKey`](ticket::TicketKey)'s.
    /// rustls's in-memory cache stays beside the tickets, as under
    /// [`Tickets`](Self::Tickets).
    SharedTickets {
        /// The keys, which the application keeps a clone of to rotate them.
        keys: TicketKeys,
        /// How long after it was issued a ticket is honoured, which is also
        /// the lifetime clients are told.
        ///
        /// At least a second and at most seven days, RFC 8446 §4.6.1's limit;
        /// anything else is [`TlsError::TicketLifetime`] when the server is
        /// [prepared](crate::server::Server::prepare). A ticket older than
        /// this is refused even while its key is accepted, and one whose key
        /// was dropped is refused however new it is. Replicas compare wall
        /// clocks, so one running slow honours a ticket for that much longer.
        lifetime: Duration,
    },
    /// No resumption: every connection pays a full handshake.
    Disabled,
}

/// TLS configuration shared by every listener.
#[derive(Debug)]
pub struct TlsConfig {
    default_certificate: CertificateMaterial,
    sni_certificates: Vec<CertificateMaterial>,
    pub(in crate::server) client_authentication: Option<ClientCertificateConfig>,
    handshake_timeout: Duration,
    session_resumption: SessionResumption,
}

impl TlsConfig {
    /// Parses a default PEM certificate chain and private key.
    pub fn from_pem(
        certificate_chain: &[u8],
        private_key: &[u8],
    ) -> std::result::Result<Self, TlsError> {
        Ok(Self {
            default_certificate: parse_certificate_material(
                Vec::new(),
                certificate_chain,
                private_key,
            )?,
            sni_certificates: Vec::new(),
            client_authentication: None,
            handshake_timeout: Duration::from_secs(10),
            session_resumption: SessionResumption::default(),
        })
    }

    /// Adds a certificate selected for any of `server_names` through SNI.
    pub fn with_server_certificate(
        mut self,
        server_names: impl IntoIterator<Item = impl Into<String>>,
        certificate_chain: &[u8],
        private_key: &[u8],
    ) -> std::result::Result<Self, TlsError> {
        let names = certificate::server_names(
            server_names.into_iter().map(Into::into),
            &self.sni_certificates,
        )?;
        self.sni_certificates.push(parse_certificate_material(
            names,
            certificate_chain,
            private_key,
        )?);
        Ok(self)
    }

    /// Requires a verified client certificate on every connection.
    #[must_use]
    pub fn require_client_certificate(mut self, config: ClientCertificateConfig) -> Self {
        self.client_authentication = Some(config);
        self
    }

    /// Sets the TLS handshake deadline.
    pub fn handshake_timeout(mut self, timeout: Duration) -> std::result::Result<Self, TlsError> {
        if timeout.is_zero() {
            return Err(TlsError::ZeroHandshakeTimeout);
        }
        self.handshake_timeout = timeout;
        Ok(self)
    }

    /// Sets how returning clients resume their sessions.
    ///
    /// [`SessionResumption::Tickets`] by default.
    #[must_use]
    pub fn session_resumption(mut self, resumption: SessionResumption) -> Self {
        self.session_resumption = resumption;
        self
    }

    pub(in crate::server) fn build(self) -> std::result::Result<TlsRuntime, TlsError> {
        let provider = crypto_provider();
        let builder = RustlsServerConfig::builder_with_provider(Arc::clone(&provider))
            .with_safe_default_protocol_versions()
            .map_err(|error| TlsError::CryptoProvider(Box::new(error)))?;
        let default = certified_key(&provider, self.default_certificate)?;
        let mut by_name = BTreeMap::new();
        for material in self.sni_certificates {
            let names = material.names.clone();
            let key = certified_key(&provider, material)?;
            for name in names {
                by_name.insert(name, Arc::clone(&key));
            }
        }
        let resolver = Arc::new(StaticCertificateResolver { default, by_name });

        let mut config = if let Some(client) = self.client_authentication {
            let mut roots = RootCertStore::empty();
            for certificate in client.roots {
                roots
                    .add(certificate)
                    .map_err(|error| TlsError::ClientVerifier(Box::new(error)))?;
            }
            let mut verifier =
                WebPkiClientVerifier::builder_with_provider(Arc::new(roots), Arc::clone(&provider));
            if !client.crls.is_empty() {
                verifier = verifier.with_crls(client.crls);
            }
            builder
                .with_client_cert_verifier(
                    verifier
                        .build()
                        .map_err(|error| TlsError::ClientVerifier(Box::new(error)))?,
                )
                .with_cert_resolver(resolver)
        } else {
            builder.with_no_client_auth().with_cert_resolver(resolver)
        };

        config.alpn_protocols = vec![
            #[cfg(feature = "http2")]
            crate::server::protocol::ALPN_HTTP2.to_vec(),
            #[cfg(feature = "http1")]
            crate::server::protocol::ALPN_HTTP1_1.to_vec(),
        ];
        config.max_early_data_size = 0;
        match self.session_resumption {
            SessionResumption::Tickets => {
                config.ticketer =
                    Ticketer::new().map_err(|error| TlsError::Ticketer(Box::new(error)))?;
            }
            SessionResumption::SharedTickets { keys, lifetime } => {
                config.ticketer = Arc::new(SharedTicketer::new(keys, lifetime, &provider)?);
            }
            SessionResumption::Cache { capacity } => {
                config.session_storage = ServerSessionMemoryCache::new(capacity.get());
            }
            SessionResumption::Disabled => {
                config.session_storage = Arc::new(NoServerSessionStorage {});
                config.send_tls13_tickets = 0;
            }
        }
        Ok(TlsRuntime {
            acceptor: tokio_rustls::TlsAcceptor::from(Arc::new(config)),
            handshake_timeout: self.handshake_timeout,
        })
    }
}

/// The crypto provider every rustls configuration here is built on.
///
/// Named rather than resolved. rustls's implicit constructors -- `ServerConfig`'s
/// and `ClientConfig`'s `builder`, and `WebPkiClientVerifier`'s -- derive the
/// process-level provider from the `aws-lc-rs` and `ring` features of whatever
/// `rustls` the graph unified on, and panic when zero or two of them are
/// compiled in. Cargo features are additive, so one dependency enabling `ring`
/// for its own reasons puts every dependent in that state and no downstream
/// manifest can leave it; the panic then lands inside [`TlsConfig::build`],
/// whose signature already carries a [`TlsError`]. Every rustls value built
/// here therefore takes this provider explicitly -- all three constructors, not
/// the two on the path without a client certificate.
///
/// A caller that installed a default still wins, which is what keeps a FIPS or
/// hardware-backed provider reachable — for everything but the default session
/// tickets, which rustls's provider interface does not carry: under
/// [`SessionResumption::Tickets`] they are sealed by `aws-lc-rs` whatever is
/// installed, and a deployment that needs every secret on its own provider
/// chooses [`SessionResumption::SharedTickets`], whose tickets that provider
/// seals, or [`SessionResumption::Cache`] or [`SessionResumption::Disabled`].
/// Otherwise the choice is Kynos's, and
/// `tokio-rustls` is declared with `default-features = false` and `aws-lc-rs`
/// named explicitly so the provider chosen here is always compiled in.
///
/// Nothing is installed as a side effect: a library that writes a process-wide
/// static takes a decision away from the binary that owns it. That is what the
/// implicit constructors do on their way through -- they install what they
/// resolved -- and it is why avoiding them matters even where the graph is
/// unambiguous and they would not have panicked.
pub(in crate::server) fn crypto_provider() -> Arc<CryptoProvider> {
    CryptoProvider::get_default().map_or_else(
        || Arc::new(tokio_rustls::rustls::crypto::aws_lc_rs::default_provider()),
        Arc::clone,
    )
}

#[derive(Clone)]
pub(in crate::server) struct TlsRuntime {
    pub(in crate::server) acceptor: tokio_rustls::TlsAcceptor,
    pub(in crate::server) handshake_timeout: Duration,
}

impl fmt::Debug for TlsRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TlsRuntime")
            .field("handshake_timeout", &self.handshake_timeout)
            .finish_non_exhaustive()
    }
}
