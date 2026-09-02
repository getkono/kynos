//! HTTP/1 and HTTP/2 tuning, and the checks that reject an unusable
//! combination before a socket is bound.

use std::time::Duration;

use crate::server::error::ServerError;

/// The smallest per-connection read/write buffer the crate accepts.
///
/// `pub(crate)` so per-connection budgets elsewhere can be measured against
/// it rather than against a figure transcribed from prose, which would drift.
#[cfg(feature = "http1")]
pub(crate) const MIN_HTTP1_BUFFER_SIZE: usize = 8_192;

/// HTTP/1 tuning.
///
/// `#[non_exhaustive]`, so it grows without breaking callers — which also means
/// a struct literal will not compile outside this crate, even with `..default()`.
/// Start from [`default`](Self::default) and set what you need:
///
/// ```
/// # use kynos::server::protocol::Http1Config;
/// let http1 = Http1Config::default().max_headers(64);
/// ```
///
/// The fields stay public because reading one is useful and never ambiguous.
/// The setters exist because writing one otherwise costs a `let mut` binding
/// and a block.
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

#[cfg(feature = "http1")]
impl Http1Config {
    /// Sets whether to keep connections alive between requests.
    #[must_use]
    pub fn keep_alive(mut self, keep_alive: bool) -> Self {
        self.keep_alive = keep_alive;
        self
    }

    /// Sets how long a client may take to send the request head.
    ///
    /// `None` waits indefinitely, which is a decision rather than a default: a
    /// client that never finishes a request head holds the connection open.
    #[must_use]
    pub fn header_read_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.header_read_timeout = timeout;
        self
    }

    /// Sets the maximum number of request headers.
    #[must_use]
    pub fn max_headers(mut self, max_headers: usize) -> Self {
        self.max_headers = max_headers;
        self
    }

    /// Sets the maximum per-connection read/write buffer size.
    #[must_use]
    pub fn max_buffer_size(mut self, max_buffer_size: usize) -> Self {
        self.max_buffer_size = max_buffer_size;
        self
    }
}

/// The HTTP/1 header cap the driver is told about.
///
/// A function rather than a branch at the call site, so the decision can be
/// asserted without a socket — which is what `AGENTS.md` means by refactoring
/// adjacent code to expose the internals a bug fix needs.
#[cfg(feature = "http1")]
pub(in crate::server) const fn forwarded_max_headers(config: &Http1Config) -> usize {
    config.max_headers
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
///
/// `#[non_exhaustive]` for the same reason as [`Http1Config`], and built the
/// same way:
///
/// ```
/// # use kynos::server::protocol::Http2Config;
/// let http2 = Http2Config::default().max_concurrent_streams(64);
/// ```
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

#[cfg(feature = "http2")]
impl Http2Config {
    /// Sets the maximum concurrent streams on one connection.
    #[must_use]
    pub fn max_concurrent_streams(mut self, streams: u32) -> Self {
        self.max_concurrent_streams = streams;
        self
    }

    /// Sets the flow-control policy.
    #[must_use]
    pub fn flow_control(mut self, flow_control: Http2FlowControl) -> Self {
        self.flow_control = flow_control;
        self
    }

    /// Sets the keep-alive policy, or `None` to send no keep-alive pings.
    #[must_use]
    pub fn keep_alive(mut self, keep_alive: Option<Http2KeepAlive>) -> Self {
        self.keep_alive = keep_alive;
        self
    }

    /// Sets the maximum decoded request header-list size.
    #[must_use]
    pub fn max_header_list_size(mut self, size: u32) -> Self {
        self.max_header_list_size = size;
        self
    }

    /// Sets the maximum buffered response bytes per stream.
    #[must_use]
    pub fn max_send_buffer_size(mut self, size: usize) -> Self {
        self.max_send_buffer_size = size;
        self
    }

    /// Sets the maximum peer-created reset streams awaiting acceptance.
    #[must_use]
    pub fn max_pending_accept_reset_streams(mut self, streams: usize) -> Self {
        self.max_pending_accept_reset_streams = streams;
        self
    }

    /// Sets the maximum locally reset streams retained before sending GOAWAY.
    #[must_use]
    pub fn max_local_error_reset_streams(mut self, streams: usize) -> Self {
        self.max_local_error_reset_streams = streams;
        self
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
