//! Serving every listener over TLS.

pub mod certificate;
pub mod document;
pub mod error;

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
    time::Duration,
};

use tokio_rustls::rustls::{
    RootCertStore, ServerConfig as RustlsServerConfig,
    pki_types::{CertificateDer, CertificateRevocationListDer, pem::PemObject},
    server::WebPkiClientVerifier,
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
                message: error.to_string(),
            })?;
        if parsed.is_empty() {
            return Err(TlsError::Pem {
                kind: "certificate revocation list",
                message: "no PEM item found".to_owned(),
            });
        }
        self.crls.extend(parsed);
        Ok(self)
    }
}

/// TLS configuration shared by every listener.
#[derive(Debug)]
pub struct TlsConfig {
    default_certificate: CertificateMaterial,
    sni_certificates: Vec<CertificateMaterial>,
    pub(in crate::server) client_authentication: Option<ClientCertificateConfig>,
    handshake_timeout: Duration,
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

    pub(in crate::server) fn build(self) -> std::result::Result<TlsRuntime, TlsError> {
        let builder = RustlsServerConfig::builder();
        let provider = builder.crypto_provider().clone();
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
                    .map_err(|error| TlsError::ClientVerifier(error.to_string()))?;
            }
            let mut verifier = WebPkiClientVerifier::builder(Arc::new(roots));
            if !client.crls.is_empty() {
                verifier = verifier.with_crls(client.crls);
            }
            builder
                .with_client_cert_verifier(
                    verifier
                        .build()
                        .map_err(|error| TlsError::ClientVerifier(error.to_string()))?,
                )
                .with_cert_resolver(resolver)
        } else {
            builder.with_no_client_auth().with_cert_resolver(resolver)
        };

        config.alpn_protocols = vec![
            #[cfg(feature = "http2")]
            b"h2".to_vec(),
            #[cfg(feature = "http1")]
            b"http/1.1".to_vec(),
        ];
        config.max_early_data_size = 0;
        Ok(TlsRuntime {
            acceptor: tokio_rustls::TlsAcceptor::from(Arc::new(config)),
            handshake_timeout: self.handshake_timeout,
        })
    }
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
