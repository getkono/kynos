//! HTTP/1 and HTTP/2 tuning, and the checks that reject an unusable
//! combination before a socket is bound.

use std::time::Duration;

use crate::server::error::ServerError;

#[cfg(feature = "http1")]
const MIN_HTTP1_BUFFER_SIZE: usize = 8_192;

/// HTTP/1 tuning.
#[cfg(feature = "http1")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Http1Config {
    /// Whether to keep connections alive between requests.
    pub keep_alive: bool,
    /// How long a client may take to send the request head.
    pub header_read_timeout: Option<Duration>,
    /// The maximum number of request headers.
    pub max_headers: usize,
    /// The maximum per-connection read/write buffer size.
    pub max_buffer_size: usize,
}

#[cfg(feature = "http1")]
impl Default for Http1Config {
    fn default() -> Self {
        Self {
            keep_alive: true,
            header_read_timeout: Some(Duration::from_secs(30)),
            max_headers: 100,
            max_buffer_size: 8_192 + 4_096 * 100,
        }
    }
}

/// HTTP/2 flow-control policy.
#[cfg(feature = "http2")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Http2FlowControl {
    /// Uses fixed initial stream and connection windows.
    Fixed {
        /// Initial per-stream flow-control window.
        initial_stream_window_size: u32,
        /// Initial connection flow-control window.
        initial_connection_window_size: u32,
    },
    /// Dynamically adjusts windows using measured bandwidth and latency.
    Adaptive,
}

/// HTTP/2 keep-alive ping policy.
#[cfg(feature = "http2")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Http2KeepAlive {
    /// Time between keep-alive pings.
    pub interval: Duration,
    /// Time allowed for acknowledgement before closing the connection.
    pub timeout: Duration,
}

/// HTTP/2 tuning.
#[cfg(feature = "http2")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Http2Config {
    /// Maximum concurrent streams on one connection.
    pub max_concurrent_streams: u32,
    /// Flow-control policy.
    pub flow_control: Http2FlowControl,
    /// Optional keep-alive policy.
    pub keep_alive: Option<Http2KeepAlive>,
    /// Maximum decoded request header-list size.
    pub max_header_list_size: u32,
    /// Maximum buffered response bytes per stream.
    pub max_send_buffer_size: usize,
    /// Maximum peer-created reset streams awaiting acceptance.
    pub max_pending_accept_reset_streams: usize,
    /// Maximum locally reset streams retained before sending GOAWAY.
    pub max_local_error_reset_streams: usize,
}

#[cfg(feature = "http2")]
impl Default for Http2Config {
    fn default() -> Self {
        Self {
            max_concurrent_streams: 200,
            flow_control: Http2FlowControl::Fixed {
                initial_stream_window_size: 1024 * 1024,
                initial_connection_window_size: 1024 * 1024,
            },
            keep_alive: None,
            max_header_list_size: 16 * 1024,
            max_send_buffer_size: 400 * 1024,
            max_pending_accept_reset_streams: 20,
            max_local_error_reset_streams: 1024,
        }
    }
}

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
