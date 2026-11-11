//! Both ends: an authority, the Ed25519 server identity it issues, Kynos's
//! TLS listener serving that identity on a loopback port as `TlsConfig`
//! configures it by default, and a client that trusts the authority.
//!
//! Apart from [the rows](super) because the two change for different reasons:
//! a row is added when what is measured changes, and this when what either
//! end offers does.

use std::any::Any;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::sync::Arc;

use kynos::server::tls::TlsConfig;
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DnType, IsCa, KeyPair, PKCS_ED25519,
    SerialNumber,
};
use tokio_rustls::rustls::{
    ClientConfig, ClientConnection, RootCertStore, client::Resumption, crypto::aws_lc_rs,
    pki_types::ServerName,
};

use super::Session;
use crate::driver::{Runner, Served};

/// The name the server's certificate is issued for, and the client's SNI.
const SERVER_NAME: &str = "localhost";

/// The protocol the client offers, so that every row is HTTP/1.1 and the
/// `connection_h2` group's `Date` encoding stays out of an exact group.
const ALPN_HTTP1: &[u8] = b"http/1.1";

/// Both certificates' serial: 20 bytes, RFC 5280's most, with a leading byte
/// that is neither zero nor has its top bit set, so DER encodes all 20 and
/// adds no sign byte, as rcgen's key-derived default does for most keys.
const SERIAL: [u8; 20] = [0x5a; 20];

/// Both ends, readied in setup and shared by every connection a row opens: a
/// server listening with an identity issued by an authority the client
/// trusts, whose configuration and ticketer the warming connection uses
/// first, so that the region's handshake is not the configuration's first.
pub struct Endpoints {
    /// Where the server listens.
    address: SocketAddr,
    client: Arc<ClientConfig>,
    /// What the server needs kept alive while it listens.
    _server: Box<dyn Any>,
}

impl std::fmt::Debug for Endpoints {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Endpoints").finish_non_exhaustive()
    }
}

impl Endpoints {
    /// Mints an authority and a server identity, serves `service` over TLS on
    /// a loopback port under `runner`, and configures a client for `session`:
    /// one that keeps tickets for [`Session::Resumed`], and one that keeps
    /// none otherwise, so that however often the server served it, its every
    /// handshake is full.
    ///
    /// The server is [`TlsConfig::from_pem`]'s default, built by the server's
    /// own `prepare`: its certificate resolver, its stateless tickets, and its
    /// handshake deadline. The identity is Ed25519, whose signature has one
    /// length, so no count moves with how a signature happened to encode.
    /// Both certificates carry a fixed serial for the same reason: rcgen's
    /// default derives it from the fresh key, and its DER encoding drops a
    /// leading zero byte for about one key in 128, which would shorten the
    /// certificate the server sends. The fixed serial encodes in the default's
    /// usual 20 bytes, so the certificate keeps that usual length.
    ///
    /// # Panics
    ///
    /// When a key, a certificate, a configuration or the listener cannot be
    /// built, which no host should make happen, or when `runner` is not a
    /// runtime.
    #[must_use]
    pub fn new(session: Session, service: Box<dyn Served>, runner: &Runner) -> Self {
        let provider = Arc::new(aws_lc_rs::default_provider());

        let mut authority = CertificateParams::new(Vec::new()).expect("no subject names");
        authority.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        authority.serial_number = Some(SerialNumber::from_slice(&SERIAL));
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
        leaf.serial_number = Some(SerialNumber::from_slice(&SERIAL));
        leaf.distinguished_name
            .push(DnType::CommonName, "kynos-profile server");
        let certificate = leaf
            .signed_by(&key, &authority)
            .expect("an issued certificate");

        let tls = TlsConfig::from_pem(certificate.pem().as_bytes(), key.serialize_pem().as_bytes())
            .expect("the server identity is usable");
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a loopback port is free");
        let address = listener
            .local_addr()
            .expect("a bound listener has an address");
        let server = service.listen(runner, listener, tls);

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
            address,
            client: Arc::new(client),
            _server: server,
        }
    }

    /// Where the server listens.
    pub(super) fn address(&self) -> SocketAddr {
        self.address
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
