//! Serving every listener over TLS.

pub mod certificate;
pub mod document;
pub mod error;

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    num::NonZeroUsize,
    sync::Arc,
    time::Duration,
};

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
/// Every server built from one [`TlsConfig`] shares one session store, so a
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionResumption {
    /// Stateless tickets: the session is sealed into a ticket the client holds,
    /// so resuming depends on nothing the server stored and no cache bounds how
    /// many clients can resume.
    ///
    /// The ticket keys are random and rotated every six hours, and a key stays
    /// accepted for one rotation after it stops issuing, so a ticket is honoured
    /// for six to twelve hours depending on when in its key's period it was
    /// issued. The keys live only in this process. Replicas behind a load
    /// balancer therefore cannot resume one another's sessions, and neither can
    /// a restarted process. Tickets use RFC 5077 §4's construction, with
    /// AES-256 and HMAC-SHA256, sealed by `aws-lc-rs` even when a caller installed
    /// another provider as the process default, because rustls's provider
    /// interface carries no ticketer.
    ///
    /// A TLS 1.2 ticket carries the session's master secret, so anyone who later
    /// obtains a ticket key — held in memory for about twelve hours — can
    /// decrypt the TLS 1.2 sessions recorded under it. TLS 1.3 resumption
    /// always runs a fresh key exchange, so its sessions keep forward secrecy.
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
        capacity: NonZeroUsize,
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
        let names = server_names
            .into_iter()
            .map(Into::into)
            .map(|name: String| name.to_ascii_lowercase())
            .collect::<Vec<_>>();
        let mut unique_names = BTreeSet::new();
        if let Some(name) = names
            .iter()
            .find(|name| !unique_names.insert((*name).clone()))
        {
            return Err(TlsError::ServerName(name.clone()));
        }
        if names.is_empty()
            || names.iter().any(String::is_empty)
            || names.iter().any(|name| {
                self.sni_certificates
                    .iter()
                    .flat_map(|certificate| &certificate.names)
                    .any(|existing| existing == name)
            })
        {
            return Err(TlsError::ServerName(
                names.first().cloned().unwrap_or_default(),
            ));
        }
        for name in &names {
            tokio_rustls::rustls::pki_types::ServerName::try_from(name.clone())
                .map_err(|_| TlsError::ServerName(name.clone()))?;
        }
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
/// hardware-backed provider reachable — for everything but session tickets,
/// which rustls's provider interface does not carry: under the default
/// [`SessionResumption::Tickets`] they are sealed by `aws-lc-rs` whatever is
/// installed, and a deployment that needs every secret on its own provider
/// chooses [`SessionResumption::Cache`] or [`SessionResumption::Disabled`]. Otherwise the choice is Kynos's, and
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
