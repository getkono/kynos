//! HTTP/1 and HTTP/2 tuning, the two ALPN identifiers naming those protocols
//! on the wire, and the checks that reject an unusable combination before a
//! socket is bound.

#[cfg(feature = "http1")]
pub mod http1;
#[cfg(feature = "http2")]
pub mod http2;

use crate::server::error::ServerError;
#[cfg(feature = "http1")]
use crate::server::protocol::http1::{Http1Config, MIN_HTTP1_BUFFER_SIZE};
#[cfg(feature = "http2")]
use crate::server::protocol::http2::{Http2Config, Http2FlowControl};

/// The ALPN protocol identifier for HTTP/2, as the IANA registry assigns it.
///
/// Named here rather than at either site that needs it, because both sides of
/// one negotiation read it: `tls` offers it to the client, and `connection`
/// compares what the handshake settled against it to pin the driver. Two
/// spellings of the same identifier would let a connection be offered a
/// protocol the driver then declines to recognise.
#[cfg(feature = "http2")]
pub(in crate::server) const ALPN_HTTP2: &[u8] = b"h2";

/// The same, for HTTP/1.1.
#[cfg(feature = "http1")]
pub(in crate::server) const ALPN_HTTP1_1: &[u8] = b"http/1.1";

pub(in crate::server) fn validate_protocol_config(
    #[cfg(feature = "http1")] http1: Http1Config,
    #[cfg(feature = "http2")] http2: Http2Config,
) -> std::result::Result<(), ServerError> {
    #[cfg(feature = "http1")]
    {
        if http1.max_headers == 0 {
            return Err(ServerError::InvalidConfiguration(
                "HTTP/1 max_headers must be non-zero",
            ));
        }
        if http1.max_buffer_size < MIN_HTTP1_BUFFER_SIZE {
            // `InvalidConfiguration` carries a `&'static str`, so the operator
            // is told the floor as a literal. This is what stops the two from
            // parting company when the constant moves.
            const _: () = assert!(
                MIN_HTTP1_BUFFER_SIZE == 8_192,
                "MIN_HTTP1_BUFFER_SIZE moved; the message below still says 8192"
            );
            return Err(ServerError::InvalidConfiguration(
                "HTTP/1 max_buffer_size must be at least 8192",
            ));
        }
        if http1
            .header_read_timeout
            .is_some_and(|timeout| timeout.is_zero())
        {
            return Err(ServerError::InvalidConfiguration(
                "HTTP/1 header_read_timeout must be non-zero when enabled",
            ));
        }
    }
    #[cfg(feature = "http2")]
    {
        if http2.max_concurrent_streams == 0
            || http2.max_header_list_size == 0
            || http2.max_send_buffer_size == 0
            || http2.max_send_buffer_size > u32::MAX as usize
            || http2.max_pending_accept_reset_streams == 0
            || http2.max_local_error_reset_streams == 0
        {
            return Err(ServerError::InvalidConfiguration(
                "HTTP/2 limits must be non-zero and fit their protocol fields",
            ));
        }
        if let Http2FlowControl::Fixed {
            initial_stream_window_size,
            initial_connection_window_size,
        } = http2.flow_control
        {
            if initial_stream_window_size == 0 || initial_connection_window_size == 0 {
                return Err(ServerError::InvalidConfiguration(
                    "HTTP/2 fixed flow-control windows must be non-zero",
                ));
            }
        }
        if http2
            .keep_alive
            .is_some_and(|keep_alive| keep_alive.interval.is_zero() || keep_alive.timeout.is_zero())
        {
            return Err(ServerError::InvalidConfiguration(
                "HTTP/2 keep-alive durations must be non-zero",
            ));
        }
    }
    Ok(())
}
