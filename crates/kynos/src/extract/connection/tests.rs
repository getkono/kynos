use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use super::{Connection, Inner, TlsIdentity};

/// Half of what makes the clone handed to every request a reference-count bump:
/// the handle is one pointer. A field added to [`Connection`] rather than to
/// `Inner` moves that field back onto the per-request path, where what it
/// copies is a peer certificate chain — the cost `docs/architecture.md` records
/// as the second of the three cheap wins.
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
/// `TlsIdentity` 72. Each is rounded up so an unrelated layout change does not
/// disable the gate — to the next multiple of 64 for `Inner`, but only to the
/// next multiple of 8 for `TlsIdentity`, where a 64-byte step would be most of
/// the type again and would absorb two more `Option<String>` fields that every
/// plaintext connection would pay for.
#[test]
fn the_inline_connection_record_stays_small() {
    let payload = size_of::<Inner>();
    let tls = size_of::<TlsIdentity>();

    assert!(
        payload <= 192,
        "Inner grew to {payload} bytes; 100k connections multiply this"
    );
    assert!(
        tls <= 80,
        "TlsIdentity grew to {tls} bytes, widening every connection including plaintext ones"
    );
}

/// The relation the absolutes above only ratchet: the inline record Kynos keeps
/// per accepted socket fits inside the smallest read/write buffer the transport
/// will accept — the two `Arc` counts included, the heap the record points at
/// excluded.
///
/// The anchor is `MIN_HTTP1_BUFFER_SIZE`, which lives in code and is enforced
/// by `validate_protocol_config`, rather than the roughly 16 KiB per live
/// connection `docs/architecture.md` attributes to hyper in "Why hyper stays".
/// It is half that prose figure, so holding against it is the stronger claim,
/// and a number in code cannot drift away from a document nothing checks it
/// against.
///
/// This is a slack gate, deliberately: 160 measured bytes against 8192 is 51x
/// of headroom, so the ceiling above is what binds in practice. What this
/// states is the design property — per-connection state stays a small fraction
/// of a transport buffer rather than a multiple of one — and it fires only if
/// that stops being true catastrophically rather than incrementally.
#[cfg(all(feature = "server", feature = "http1"))]
#[test]
fn per_connection_state_fits_inside_the_smallest_transport_buffer() {
    let allocation = size_of::<Inner>() + 2 * size_of::<usize>();
    let floor = crate::server::protocol::MIN_HTTP1_BUFFER_SIZE;

    assert!(
        allocation < floor,
        "per-connection state ({allocation} bytes, Inner plus the two Arc counts) \
         must stay under the smallest transport buffer the crate accepts ({floor} bytes)"
    );
}
