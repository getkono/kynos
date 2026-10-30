//! HTTP/1 and HTTP/2 tuning, the two ALPN identifiers naming those protocols
//! on the wire, and the checks that reject an unusable combination before a
//! socket is bound.

#[cfg(feature = "http1")]
pub mod http1;
#[cfg(feature = "http2")]
pub mod http2;

use std::time::Duration;

use crate::server::error::ServerError;
#[cfg(feature = "http1")]
use crate::server::protocol::http1::{Http1Config, MIN_HTTP1_BUFFER_SIZE};
#[cfg(feature = "http2")]
use crate::server::protocol::http2::{Http2Config, Http2FlowControl};

/// The ALPN protocol identifier for HTTP/2, as the IANA registry assigns it.
/// Shared by `tls`, which offers it, and `connection`, which pins on it.
#[cfg(feature = "http2")]
pub(in crate::server) const ALPN_HTTP2: &[u8] = b"h2";

/// The same, for HTTP/1.1.
#[cfg(feature = "http1")]
pub(in crate::server) const ALPN_HTTP1_1: &[u8] = b"http/1.1";

/// The largest flow-control window RFC 9113 §6.9.1 allows, 2^31-1.
///
/// `h2` asserts it while handshaking, so a larger window panics the connection
/// rather than failing `prepare`.
#[cfg(feature = "http2")]
const MAX_HTTP2_WINDOW_SIZE: u32 = 2_147_483_647;

pub(in crate::server) fn validate_protocol_config(
    #[cfg(feature = "http1")] http1: Http1Config,
    #[cfg(feature = "http2")] http2: Http2Config,
) -> std::result::Result<(), ServerError> {
    #[cfg(feature = "http1")]
    http1.validate()?;
    #[cfg(feature = "http2")]
    http2.validate()?;
    Ok(())
}

/// Refuses an enabled timeout of zero, which would expire before anything
/// could arrive, with `refusal`.
///
/// The caller constructs `refusal`, so every refusal stays a literal the
/// crate-wide refusal list reads.
pub(in crate::server) fn non_zero(
    timeout: Option<Duration>,
    refusal: ServerError,
) -> std::result::Result<(), ServerError> {
    if timeout.is_some_and(|timeout| timeout.is_zero()) {
        return Err(refusal);
    }
    Ok(())
}

#[cfg(feature = "http1")]
impl Http1Config {
    fn validate(&self) -> std::result::Result<(), ServerError> {
        if self.max_headers == 0 {
            return Err(ServerError::InvalidConfiguration(
                "HTTP/1 max_headers must be non-zero",
            ));
        }
        if self.max_buffer_size < MIN_HTTP1_BUFFER_SIZE {
            // Keeps the literal in the message in step with the constant.
            const _: () = assert!(
                MIN_HTTP1_BUFFER_SIZE == 8_192,
                "MIN_HTTP1_BUFFER_SIZE moved; the message below still says 8192"
            );
            return Err(ServerError::InvalidConfiguration(
                "HTTP/1 max_buffer_size must be at least 8192",
            ));
        }
        non_zero(
            self.header_read_timeout,
            ServerError::InvalidConfiguration(
                "HTTP/1 header_read_timeout must be non-zero when enabled",
            ),
        )
    }
}

#[cfg(feature = "http2")]
impl Http2Config {
    fn validate(&self) -> std::result::Result<(), ServerError> {
        if self.max_concurrent_streams == 0
            || self.max_header_list_size == 0
            || self.max_send_buffer_size == 0
            || self.max_send_buffer_size > u32::MAX as usize
            || self.max_pending_accept_reset_streams == 0
            || self.max_local_error_reset_streams == 0
        {
            return Err(ServerError::InvalidConfiguration(
                "HTTP/2 limits must be non-zero and fit their protocol fields",
            ));
        }
        if let Http2FlowControl::Fixed {
            initial_stream_window_size,
            initial_connection_window_size,
        } = self.flow_control
        {
            if initial_stream_window_size == 0 || initial_connection_window_size == 0 {
                return Err(ServerError::InvalidConfiguration(
                    "HTTP/2 fixed flow-control windows must be non-zero",
                ));
            }
            if initial_stream_window_size > MAX_HTTP2_WINDOW_SIZE
                || initial_connection_window_size > MAX_HTTP2_WINDOW_SIZE
            {
                return Err(ServerError::InvalidConfiguration(
                    "HTTP/2 fixed flow-control windows must not exceed 2147483647",
                ));
            }
        }
        if self
            .keep_alive
            .is_some_and(|keep_alive| keep_alive.interval.is_zero() || keep_alive.timeout.is_zero())
        {
            return Err(ServerError::InvalidConfiguration(
                "HTTP/2 keep-alive durations must be non-zero",
            ));
        }
        non_zero(
            self.idle_timeout,
            ServerError::InvalidConfiguration("HTTP/2 idle_timeout must be non-zero when enabled"),
        )
    }
}
