//! Serving over TLS, including client certificates, SNI and protocol tuning.
//!
//! Run it without the macros or the JSON codec — the transport is the subject
//! here, not the API:
//!
//! ```text
//! cargo run -p kynos --example tls --no-default-features \
//!   --features openapi31,server,http1,http2,tls
//! ```
//!
//! The certificates are minted at startup with `rcgen`, so this runs with
//! nothing prepared and nothing committed. A real deployment reads PEM from
//! disk, a secret manager or an ACME client; `from_pem` takes bytes and does
//! not care where they came from.
//!
//! The server only completes a handshake with a client presenting a
//! certificate its partner authority issued, so the example mints one, writes
//! it and its key to the system's temporary directory, and prints the `curl`
//! line that presents them. `--insecure` is there because the server's own
//! certificate is self-signed; it relaxes what curl checks of the server, not
//! what the server checks of curl.
//!
//! Worth noticing:
//!
//! * **rustls is the only TLS backend, and that is a decision rather than a
//!   default.** There is no `native-tls` feature and no runtime selection, so
//!   there is one code path to audit and one set of failure modes to
//!   understand. See `docs/architecture.md`.
//! * **SNI is a list of certificates, not a list of servers.** One listener
//!   serves several names by choosing a certificate during the handshake, which
//!   is why the names are attached to the certificate rather than to the bind
//!   address.
//! * **Client certificates are `require_`, not `allow_`.** Optional mutual TLS
//!   is a configuration that looks secure and is not: a request that arrives
//!   without a certificate would be served anyway. The scheme to *describe* it
//!   is `#[security(mutual_tls)]` — see [`security_schemes.rs`](security_schemes.rs).
//! * **`handshake_timeout` is fallible.** Zero is rejected rather than accepted
//!   as "no timeout", because a handshake that never completes is the cheapest
//!   way to hold a connection open forever.
//! * **Resumption is set here only because this server requires client
//!   certificates.** Stateless tickets, the default, let a returning client
//!   skip the full handshake; they also let a resumed session keep a
//!   certificate that has since expired, which a partner API may not accept.
//!   A fleet that does want resumption shares its ticket keys, or each replica
//!   resumes only what it issued: `SessionResumption::SharedTickets`, whose
//!   `TicketKeys` carries a runnable rotation.
//! * **Both protocol configs are set here because ALPN is where the choice is
//!   made.** Under TLS the client and server negotiate `h2` or `http/1.1`
//!   during the handshake, so a server offering both needs both configured.
//!   Without TLS there is no negotiation: `Server::http2` alone serves
//!   cleartext HTTP/2 — h2c — which clients reach only by prior knowledge.
//!
//! `prepare` splits binding from serving, which is what lets a test learn the
//! port before any request is sent — `local_addrs` is meaningless before the
//! bind and unavailable after `serve` takes ownership.

use std::{fs, net::Ipv4Addr, num::NonZeroUsize, time::Duration};

use kynos::{
    Router,
    server::{
        Server,
        protocol::{http1::Http1Config, http2::Http2Config},
        tls::{ClientCertificateConfig, SessionResumption, TlsConfig, error::TlsError},
    },
};

/// A certificate and its private key, both PEM-encoded.
struct Material {
    certificate: String,
    key: String,
}

/// Mints a self-signed certificate for `names`.
///
/// A real service does not do this. It is here so the example runs with nothing
/// prepared, which matters more for a transport example than for any other:
/// the thing worth seeing is the handshake succeeding.
fn self_signed(names: &[&str]) -> Material {
    let certified = rcgen::generate_simple_self_signed(
        names
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>(),
    )
    .expect("a self-signed certificate");

    Material {
        certificate: certified.cert.pem(),
        key: certified.signing_key.serialize_pem(),
    }
}

/// Mints a partner certificate authority and one client certificate it issues,
/// returning the authority's certificate and the client's material.
///
/// The server trusts the authority, never the client certificate itself: a
/// partner rotating its client certificate must not need a server redeploy.
fn partner_authority() -> (String, Material) {
    use rcgen::{
        BasicConstraints, CertificateParams, CertifiedIssuer, DnType, ExtendedKeyUsagePurpose,
        IsCa, KeyPair, KeyUsagePurpose,
    };

    let mut authority = CertificateParams::new(Vec::new()).expect("no subject alternative names");
    authority.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    authority.key_usages = vec![KeyUsagePurpose::KeyCertSign];
    authority
        .distinguished_name
        .push(DnType::CommonName, "Partner CA");
    let issuer = CertifiedIssuer::self_signed(authority, KeyPair::generate().expect("a key pair"))
        .expect("a self-signed authority");

    let mut client = CertificateParams::new(vec!["partner.example.test".to_owned()])
        .expect("a usable subject alternative name");
    client.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    client.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    client
        .distinguished_name
        .push(DnType::CommonName, "Partner client");
    let key = KeyPair::generate().expect("a key pair");
    let certificate = client
        .signed_by(&key, &issuer)
        .expect("a client certificate the authority issued");

    (
        issuer.pem(),
        Material {
            certificate: certificate.pem(),
            key: key.serialize_pem(),
        },
    )
}

/// Assembles the TLS configuration.
///
/// Its own function because the builder is a chain: every step takes `self` and
/// returns `Result<Self, TlsError>`, so the whole thing is one expression and
/// wants one place to be. `main` could `?` each step directly — `kynos::Error`
/// converts from a `TlsError` — but it would read as five statements instead of
/// the single configuration it is.
fn tls_config(
    default: &Material,
    tenant: &Material,
    partner_authority: &str,
) -> Result<TlsConfig, TlsError> {
    TlsConfig::from_pem(default.certificate.as_bytes(), default.key.as_bytes())?
        // One listener, several names. The certificate carries the names it is
        // valid for, so the handshake can choose without a second bind.
        .with_server_certificate(
            ["a.example.test", "b.example.test"],
            tenant.certificate.as_bytes(),
            tenant.key.as_bytes(),
        )?
        // Mutual TLS. A connection presenting no certificate, or one this CA
        // did not issue, does not reach a handler at all.
        .require_client_certificate(ClientCertificateConfig::from_pem_roots(
            partner_authority.as_bytes(),
        )?)
        // Fallible: zero is rejected rather than read as "wait forever".
        .handshake_timeout(Duration::from_secs(5))
        // Resumption is stateless tickets by default. A resumed session keeps
        // the client certificate its full handshake verified without checking
        // it again, so a partner API that must hold every connection to the
        // certificate's expiry pays a full handshake each time instead.
        .map(|config| config.session_resumption(SessionResumption::Disabled))
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let default = self_signed(&["localhost"]);
    let tenant = self_signed(&["a.example.test", "b.example.test"]);
    // In a real deployment the partner keeps its client key and sends only the
    // CA's certificate, which is emphatically not the one that issued the
    // server's.
    let (partner_ca, client) = partner_authority();

    let tls = tls_config(&default, &tenant, &partner_ca)?;

    // Only so a client can present it: a server never holds a client's key.
    let directory = std::env::temp_dir();
    let client_certificate = directory.join("kynos-tls-client.pem");
    let client_key = directory.join("kynos-tls-client-key.pem");
    fs::write(&client_certificate, &client.certificate).expect("a writable temporary directory");
    fs::write(&client_key, &client.key).expect("a writable temporary directory");

    let server = Server::new(Router::<()>::new().build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 8443))
        .tls(tls)
        // `Http1Config` is `#[non_exhaustive]`, so it grows without breaking
        // callers -- which is why it is built from `default` and a setter rather
        // than from a struct literal.
        .http1(Http1Config::default().keep_alive(true))
        // The other half of the ALPN offer. `max_concurrent_streams` is the
        // HTTP/2 analogue of `max_connections` below: one connection carries
        // many requests, so capping connections alone caps nothing.
        .http2(Http2Config::default().max_concurrent_streams(256))
        // A ceiling on accepted connections, which is the backstop a timeout is
        // not: a slow client holds a slot, and this bounds how many slots there
        // are.
        .max_connections(NonZeroUsize::new(1_024).expect("non-zero"));

    // Bound, not yet serving. This is the split that lets a test learn the port
    // it actually got when it asked for zero.
    let bound = server.prepare().await?;
    for address in bound.local_addrs() {
        println!("listening on https://{address}");
    }
    // No route is registered, so the handshake succeeding is the result, and
    // the answer is the problem document a router with no `GET` operation
    // sends. Drop `--cert` and `--key` and the handshake itself is refused.
    println!(
        "try: curl --insecure --cert {} --key {} https://localhost:8443/",
        client_certificate.display(),
        client_key.display(),
    );

    bound.serve().await
}
