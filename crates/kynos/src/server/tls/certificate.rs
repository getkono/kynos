//! Parsing certificate material, and resolving it per SNI name.

use std::{collections::BTreeMap, sync::Arc};

use tokio_rustls::rustls::{
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    server::{ClientHello, ResolvesServerCert},
    sign::CertifiedKey,
};

use crate::server::tls::error::TlsError;

#[derive(Debug)]
pub(in crate::server) struct CertificateMaterial {
    pub(in crate::server) names: Vec<String>,
    pub(in crate::server) certificates: Vec<CertificateDer<'static>>,
    pub(in crate::server) private_key: PrivateKeyDer<'static>,
}

pub(in crate::server) fn parse_certificates(
    bytes: &[u8],
    kind: &'static str,
) -> std::result::Result<Vec<CertificateDer<'static>>, TlsError> {
    let certificates = CertificateDer::pem_slice_iter(bytes)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| TlsError::Pem {
            kind,
            message: error.to_string(),
        })?;
    if certificates.is_empty() {
        return Err(TlsError::Pem {
            kind,
            message: "no PEM item found".to_owned(),
        });
    }
    Ok(certificates)
}

pub(in crate::server) fn parse_certificate_material(
    names: Vec<String>,
    certificate_chain: &[u8],
    private_key: &[u8],
) -> std::result::Result<CertificateMaterial, TlsError> {
    let certificates = parse_certificates(certificate_chain, "certificate")?;
    let private_key =
        PrivateKeyDer::from_pem_slice(private_key).map_err(|error| TlsError::Pem {
            kind: "private key",
            message: error.to_string(),
        })?;
    Ok(CertificateMaterial {
        names,
        certificates,
        private_key,
    })
}

pub(in crate::server) fn certified_key(
    provider: &tokio_rustls::rustls::crypto::CryptoProvider,
    material: CertificateMaterial,
) -> std::result::Result<Arc<CertifiedKey>, TlsError> {
    let key = provider
        .key_provider
        .load_private_key(material.private_key)
        .map_err(|error| TlsError::PrivateKey(error.to_string()))?;
    let certified = CertifiedKey::new(material.certificates, key);
    certified
        .keys_match()
        .map_err(|error| TlsError::PrivateKey(error.to_string()))?;
    Ok(Arc::new(certified))
}

#[derive(Debug)]
pub(in crate::server) struct StaticCertificateResolver {
    pub(in crate::server) default: Arc<CertifiedKey>,
    pub(in crate::server) by_name: BTreeMap<String, Arc<CertifiedKey>>,
}

impl ResolvesServerCert for StaticCertificateResolver {
    fn resolve(&self, hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        hello
            .server_name()
            .and_then(|name| self.by_name.get(&name.to_ascii_lowercase()))
            .cloned()
            .or_else(|| Some(Arc::clone(&self.default)))
    }
}
