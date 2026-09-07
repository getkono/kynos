use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use super::{Connection, Inner, TlsIdentity};

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
/// so a three-certificate mTLS chain — roughly 4.6 KiB of DER — moves none of
/// these readings. `docs/architecture.md` records that chain among what one
/// accepted socket costs and nothing bounds.
///
/// `Inner` carries `Option<TlsIdentity>` at every feature set, including a build
/// with no TLS at all, so a field added to the TLS half widens every plaintext
/// connection too.
///
/// The measurements these ceilings were set from, since
/// `docs/nfr.md#thresholds` asks for a recorded one: `Inner` 144 bytes,
/// `TlsIdentity` 72. Each is rounded up to the next multiple of 64 so an
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
