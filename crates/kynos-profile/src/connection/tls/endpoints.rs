//! Both ends' TLS configuration: an authority, the Ed25519 server identity
//! it issues, the server's configuration as `TlsConfig` builds it by default,
//! and a client that trusts the authority.
//!
//! Apart from [the rows](super) because the two change for different reasons:
//! a row is added when what is measured changes, and this when what either
//! end offers does.

use std::sync::Arc;

use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DnType, IsCa, KeyPair, PKCS_ED25519,
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        ClientConfig, ClientConnection, RootCertStore, ServerConfig,
        client::Resumption,
        crypto::aws_lc_rs,
        pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
    },
};

use super::Session;

/// The name the server's certificate is issued for, and the client's SNI.
const SERVER_NAME: &str = "localhost";

/// The protocol the client offers, so that every row is HTTP/1.1 and the
/// `connection_h2` group's `Date` encoding stays out of an exact group.
const ALPN_HTTP1: &[u8] = b"http/1.1";

/// Both ends' configuration, minted in setup and shared by every connection
/// a row opens: a server identity issued by an authority the client trusts,
/// and the server's ticketer, which the warming connection uses first, so
/// that the region's handshake is not the configuration's first.
pub struct Endpoints {
    server: Arc<ServerConfig>,
    client: Arc<ClientConfig>,
}

impl std::fmt::Debug for Endpoints {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Endpoints").finish_non_exhaustive()
    }
}

impl Endpoints {
    /// Mints an authority and a server identity, and configures both ends for
    /// `session`: a client that keeps tickets for [`Session::Resumed`], and
    /// one that keeps none otherwise, so that however often the server served
    /// it, its every handshake is full.
    ///
    /// The server is `TlsConfig`'s default: `aws-lc-rs`, TLS 1.2 and 1.3,
    /// ALPN `h2` then `http/1.1`, no early data, and stateless tickets under
    /// `aws-lc-rs`'s ticketer. The identity is Ed25519, whose signature has
    /// one length, so no count moves with how a signature happened to encode.
    ///
    /// # Panics
    ///
    /// When a key, a certificate or a configuration cannot be built, which
    /// no host should make happen.
    #[must_use]
    pub fn new(session: Session) -> Self {
        let provider = Arc::new(aws_lc_rs::default_provider());

        let mut authority = CertificateParams::new(Vec::new()).expect("no subject names");
        authority.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        authority
            .distinguished_name
            .push(DnType::CommonName, "kynos-profile authority");
        let authority = CertifiedIssuer::self_signed(
            authority,
            KeyPair::generate_for(&PKCS_ED25519).expect("an Ed25519 key"),
        )
        .expect("a self-signed authority");
        let key = KeyPair::generate_for(&PKCS_ED25519).expect("an Ed25519 key");
        let mut leaf =
            CertificateParams::new(vec![SERVER_NAME.to_owned()]).expect("a valid subject name");
        leaf.distinguished_name
            .push(DnType::CommonName, "kynos-profile server");
        let certificate = leaf
            .signed_by(&key, &authority)
            .expect("an issued certificate");

        let mut server = ServerConfig::builder_with_provider(Arc::clone(&provider))
            .with_safe_default_protocol_versions()
            .expect("aws-lc-rs supports the default versions")
            .with_no_client_auth()
            .with_single_cert(
                vec![certificate.der().clone()],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
            )
            .expect("the server identity is usable");
        server.alpn_protocols = vec![b"h2".to_vec(), ALPN_HTTP1.to_vec()];
        server.max_early_data_size = 0;
        server.ticketer = aws_lc_rs::Ticketer::new().expect("a ticketer");

        let mut roots = RootCertStore::empty();
        roots
            .add(authority.der().clone())
            .expect("the authority is a usable trust anchor");
        let mut client = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("aws-lc-rs supports the default versions")
            .with_root_certificates(roots)
            .with_no_client_auth();
        client.alpn_protocols = vec![ALPN_HTTP1.to_vec()];
        if session != Session::Resumed {
            client.resumption = Resumption::disabled();
        }

        Self {
            server: Arc::new(server),
            client: Arc::new(client),
        }
    }

    /// The server's acceptor, sharing its configuration with every other.
    pub(super) fn acceptor(&self) -> TlsAcceptor {
        TlsAcceptor::from(Arc::clone(&self.server))
    }

    /// A new client session, which takes a ticket from the store where its
    /// configuration keeps one.
    pub(super) fn client(&self) -> ClientConnection {
        ClientConnection::new(
            Arc::clone(&self.client),
            ServerName::try_from(SERVER_NAME).expect("a DNS name"),
        )
        .expect("a client session")
    }
}
