use super::{Connection, Inner, TlsIdentity};

/// The clone handed to every request on a connection is a reference-count bump
/// only because the handle is one pointer. A field added to [`Connection`]
/// rather than to `Inner` moves that field back onto the per-request path,
/// where what it copies is a peer certificate chain — the cost
/// `docs/architecture.md` records as the second of the three cheap wins.
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

/// What one accepted socket costs on the heap, guarded as two ceilings rather
/// than one. `Inner` carries `Option<TlsIdentity>` at every feature set,
/// including a build with no TLS at all, so a field added to the TLS half
/// widens every plaintext connection too.
///
/// Both bounds are ceilings, not targets: they were measured and rounded up, as
/// `docs/nfr.md#thresholds` requires, so an unrelated layout change does not
/// disable the gate.
#[test]
fn a_connection_costs_one_small_allocation() {
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

/// The relation the absolutes above only ratchet: everything Kynos itself keeps
/// per accepted socket fits inside the smallest read/write buffer the transport
/// will accept — the two `Arc` counts included.
///
/// The anchor is [`MIN_HTTP1_BUFFER_SIZE`](crate::server::protocol), which lives
/// in code and is enforced by `validate_protocol_config`, rather than the
/// roughly 16 KiB per live connection `docs/architecture.md` attributes to hyper
/// in "Why hyper stays". It is half that prose figure, so holding against it is
/// the stronger claim, and a number in code cannot drift away from a document
/// nothing checks it against.
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
