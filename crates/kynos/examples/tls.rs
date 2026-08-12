//! Serving over TLS, including client certificates and SNI.
//!
//! Run it without the macros or the JSON codec — the transport is the subject
//! here, not the API:
//!
//! ```text
//! cargo run -p kynos --example tls --no-default-features \
//!   --features openapi31,server,http1,tls
//! ```
//!
//! The certificates are minted at startup with `rcgen`, so this runs with
//! nothing prepared and nothing committed. A real deployment reads PEM from
//! disk, a secret manager or an ACME client; `from_pem` takes bytes and does
//! not care where they came from.
//!
//! Four things are worth noticing:
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
//!
//! `prepare` splits binding from serving, which is what lets a test learn the
//! port before any request is sent — `local_addrs` is meaningless before the
//! bind and unavailable after `serve` takes ownership.

use std::{net::Ipv4Addr, num::NonZeroUsize, time::Duration};

use kynos::{
    Router,
    server::{
        Server,
        error::ServerError,
        protocol::Http1Config,
        tls::{ClientCertificateConfig, TlsConfig, error::TlsError},
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

/// Assembles the TLS configuration.
///
/// Its own function because every step returns `TlsError`, and gathering them
/// here means one conversion at the call site rather than one per line.
fn tls_config(
    default: &Material,
    tenant: &Material,
    partners: &Material,
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
            partners.certificate.as_bytes(),
        )?)
        // Fallible: zero is rejected rather than read as "wait forever".
        .handshake_timeout(Duration::from_secs(5))
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let default = self_signed(&["localhost"]);
    let tenant = self_signed(&["a.example.test", "b.example.test"]);
    // In a real deployment this is the CA that issued the partner's client
    // certificates, and it is emphatically not the same one that issued the
    // server's.
    let partners = self_signed(&["partner-ca.example.test"]);

    let tls = tls_config(&default, &tenant, &partners).map_err(ServerError::from)?;

    let server = Server::new(Router::<()>::new().build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 8443))
        .tls(tls)
        .http1({
            // `#[non_exhaustive]`, so it grows without breaking callers --
            // which also means starting from `default` rather than a literal.
            let mut http1 = Http1Config::default();
            http1.keep_alive = true;
            http1
        })
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

    bound.serve().await
}
