use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use super::{ConnectInfo, Connection, Inner, TlsIdentity, TransportAddress};
use crate::{error::rejection::ConnectInfoRejection, extract::FromRequestParts};

/// What `ConnectInfo` extracts from a request that arrived on `connection`, or
/// on no socket at all.
async fn connect_info(connection: Option<Connection>) -> Result<ConnectInfo, ConnectInfoRejection> {
    let mut parts = http::Request::new(()).into_parts().0;
    if let Some(connection) = connection {
        parts.extensions.insert(connection);
    }
    ConnectInfo::from_request_parts(&mut parts, &()).await
}

#[tokio::test]
async fn an_ip_connection_reports_its_peer_through_every_reader() {
    let peer = SocketAddr::from((Ipv4Addr::new(192, 0, 2, 1), 50_000));
    let local = SocketAddr::from((Ipv4Addr::new(192, 0, 2, 2), 80));
    let connection = Connection::from_peer(peer, local);

    assert!(matches!(connection.peer(), TransportAddress::Ip(at) if *at == peer));
    assert!(matches!(connection.local(), TransportAddress::Ip(at) if *at == local));
    assert_eq!(connection.peer_addr(), Some(peer));
    assert_eq!(connection.local_addr(), Some(local));
    assert_eq!(connect_info(Some(connection)).await, Ok(ConnectInfo(peer)));
}

/// No socket is not a transport without IP: the in-process sentinel stays an
/// IP address, so a handler taking `ConnectInfo` still runs under `TestClient`.
#[tokio::test]
async fn a_request_on_no_socket_reports_the_in_process_address() {
    let unspecified = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0));

    assert_eq!(connect_info(None).await, Ok(ConnectInfo(unspecified)));
}

/// A Unix peer has no IP address, so `ConnectInfo` refuses rather than
/// inventing one, and the IP readers say `None` while the transport readers
/// keep the address.
#[cfg(unix)]
#[tokio::test]
async fn a_unix_connection_has_no_ip_peer_for_connect_info() {
    let (stream, _other) = std::os::unix::net::UnixStream::pair().expect("a socket pair");
    let connection = Connection::from_peer(
        stream.peer_addr().expect("a peer address"),
        stream.local_addr().expect("a local address"),
    );

    assert!(matches!(
        connection.peer(),
        TransportAddress::Unix(address) if address.as_std().is_unnamed()
    ));
    assert!(matches!(connection.local(), TransportAddress::Unix(_)));
    assert_eq!(connection.peer_addr(), None);
    assert_eq!(connection.local_addr(), None);
    assert!(!connection.is_in_process());
    assert_eq!(
        connect_info(Some(connection)).await,
        Err(ConnectInfoRejection::NotIp)
    );
}

/// A pathname address survives the record, so a handler can tell which socket
/// file a connection was accepted on.
#[cfg(unix)]
#[test]
fn a_unix_pathname_is_kept_as_given() {
    let path = std::path::Path::new("/run/kynos/api.sock");
    let named = std::os::unix::net::SocketAddr::from_pathname(path).expect("a short path");
    let unnamed = std::os::unix::net::UnixStream::pair()
        .expect("a socket pair")
        .0
        .peer_addr()
        .expect("a peer address");
    let connection = Connection::from_peer(unnamed, named);

    assert!(matches!(
        connection.local(),
        TransportAddress::Unix(address) if address.as_std().as_pathname() == Some(path)
    ));
}

/// Half of what makes the clone handed to every request a reference-count bump:
/// the handle is one pointer. A field added to [`Connection`] rather than to
/// `Inner` moves that field back onto the per-request path, where what it
/// copies is a peer certificate chain — the cost `docs/architecture.md` records
/// as the second of the three cheap wins.
///
/// The pin is also where the handle is held narrower than the payload behind
/// the `Arc`. That relation is not entailed by the ceiling below: `Inner <= 192`
/// is an upper bound, and the relation needs a lower one — shrink `Inner` to a
/// single `bool` and every other assertion here still passes while the payload
/// is the narrower of the two. It is asserted rather than stated.
///
/// The other half is `a_connection_clone_shares_its_payload` below, which is
/// the assertion that rules out `Box`.
#[test]
fn a_connection_handle_is_one_pointer() {
    let handle = size_of::<Connection>();
    let payload = size_of::<Inner>();

    assert_eq!(
        handle,
        size_of::<usize>(),
        "Connection ({handle} bytes) must stay one pointer wide; \
         a field added here is copied per request rather than per connection"
    );
    assert!(
        handle < payload,
        "Connection ({handle} bytes) should stay smaller than Inner ({payload} bytes); \
         the payload belongs behind the Arc"
    );
}

/// Nothing records credentials but a Unix listener or an embedding that read
/// them: a socket recorded by address alone, and no socket at all, report none.
#[cfg(unix)]
#[test]
fn a_connection_reports_no_credentials_unless_some_were_recorded() {
    let (stream, _other) = std::os::unix::net::UnixStream::pair().expect("a socket pair");
    let unix = Connection::from_peer(
        stream.peer_addr().expect("a peer address"),
        stream.local_addr().expect("a local address"),
    );
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, 8080));

    assert_eq!(unix.peer_credentials(), None);
    assert_eq!(
        Connection::from_peer(address, address).peer_credentials(),
        None
    );
    let parts = http::Request::new(()).into_parts().0;
    assert_eq!(Connection::of(&parts).peer_credentials(), None);
}

/// Recording credentials on a handle already shared leaves the other handles,
/// and the in-process connection every unsocketed request shares, as they
/// were.
#[cfg(unix)]
#[test]
fn recorded_credentials_reach_only_the_handle_they_were_recorded_on() {
    use super::PeerCredentials;

    let credentials = PeerCredentials::new(1000, 100).with_pid(4242);
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, 8080));
    let shared = Connection::from_peer(address, address);
    let other = shared.clone();

    let recorded = shared.with_peer_credentials(credentials);
    assert_eq!(recorded.peer_credentials(), Some(credentials));
    assert_eq!(recorded.peer_addr(), Some(address));
    assert_eq!(other.peer_credentials(), None);

    let parts = http::Request::new(()).into_parts().0;
    let in_process = Connection::of(&parts).with_peer_credentials(credentials);
    assert_eq!(in_process.peer_credentials(), Some(credentials));
    assert!(in_process.is_in_process());
    assert_eq!(Connection::of(&parts).peer_credentials(), None);
}

/// A transport address is as wide as the IP address it most often holds: the
/// Unix variant is held out of line, so an IP connection's record does not
/// grow to the width of a `sockaddr_un`, which is what keeps `Inner` at the
/// 176 bytes measured below.
#[test]
fn a_transport_address_is_as_wide_as_an_ip_one() {
    assert_eq!(
        size_of::<TransportAddress>(),
        size_of::<SocketAddr>(),
        "a variant held inline widens both addresses of every connection"
    );
}

/// The other half: cloning shares the payload rather than copying it.
///
/// One pointer wide is necessary and not sufficient. `Connection(Box<Inner>)`
/// measures the same 8 bytes, satisfies both assertions above, and deep-copies
/// a peer certificate chain into every request on the connection — which is
/// precisely the cost `docs/architecture.md` records as taken. Only pointer
/// identity separates the two shapes.
#[test]
fn a_connection_clone_shares_its_payload() {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, 8080));
    let connection = Connection::from_peer(address, address);

    let one = connection.clone();
    let two = connection.clone();

    assert!(
        Arc::ptr_eq(&one.0, &two.0),
        "two clones of one Connection must point at the same Inner; \
         a handle that copies its payload copies a certificate chain per request"
    );
}

/// What one accepted socket costs *inline*, guarded as two ceilings rather than
/// one. `size_of` counts the record and the headers within it, never the bytes
/// a header points at: `server_name`, `alpn` and `peer_certificates` are three
/// pointer-width triples here and an unbounded number of kilobytes on the heap,
/// so a three-certificate mTLS chain — kilobytes of DER these readings do not
/// see — moves none of them. `docs/architecture.md` records that chain among what one
/// accepted socket costs and nothing bounds.
///
/// `Inner` carries `Option<TlsIdentity>` at every feature set, including a build
/// with no TLS at all, so a field added to the TLS half widens every plaintext
/// connection too. On Unix it carries `Option<PeerCredentials>` the same way, so
/// a TCP connection pays its 16 bytes as well.
///
/// The measurements these ceilings were set from, since
/// `docs/nfr.md#thresholds` asks for a recorded one: `Inner` 176 bytes on
/// 64-bit Linux with `server` (160 without), `TlsIdentity` 72. Each is rounded up to the next multiple of 64 so an
/// unrelated layout change does not disable the gate, which is the step every
/// other per-connection ceiling in the crate uses. A tighter step on
/// `TlsIdentity` would bind first on a toolchain that reorders one field, and
/// `docs/performance.md#thresholds` records what happens to a gate that fires
/// for a reason its design is not about.
///
/// Both readings are far under the smallest read/write buffer the transport
/// accepts, which is the design property: per-connection state is a fraction of
/// a transport buffer rather than a multiple of one. That is prose here because
/// nothing can falsify it while these ceilings hold —
/// `docs/architecture.md` records it in "Why hyper stays".
#[test]
fn the_inline_connection_record_stays_small() {
    let payload = size_of::<Inner>();
    let tls = size_of::<TlsIdentity>();

    assert!(
        payload <= 192,
        "Inner grew to {payload} bytes; 100k connections multiply this"
    );
    assert!(
        tls <= 128,
        "TlsIdentity grew to {tls} bytes, widening every connection including plaintext ones"
    );
}
