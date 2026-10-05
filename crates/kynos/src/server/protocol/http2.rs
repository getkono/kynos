//! HTTP/2 tuning.

use std::time::Duration;

/// HTTP/2 flow-control policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Http2FlowControl {
    /// Uses fixed initial stream and connection windows.
    ///
    /// Each window must lie in `1..=2^31-1`, the range RFC 9113 §6.9.1 allows
    /// a flow-control window; [`Server::prepare`](crate::server::Server::prepare)
    /// refuses any other with
    /// [`ServerError::InvalidConfiguration`](crate::server::error::ServerError::InvalidConfiguration).
    Fixed {
        /// Initial per-stream flow-control window, in `1..=2^31-1`.
        initial_stream_window_size: u32,
        /// Initial connection flow-control window, in `1..=2^31-1`.
        initial_connection_window_size: u32,
    },
    /// Dynamically adjusts windows using measured bandwidth and latency.
    Adaptive,
}

/// HTTP/2 keep-alive ping policy.
///
/// A PING is sent once a connection has read nothing for `interval`, and the
/// connection is closed if it is not acknowledged within `timeout`, so a busy
/// connection is never pinged. This is what releases an HTTP/2 connection
/// whose peer vanished with no stream open: HTTP/1's header-read timeout has no
/// counterpart there.
///
/// `#[non_exhaustive]`, so construct it with [`new`](Self::new):
///
/// ```
/// # use std::time::Duration;
/// # use kynos::server::protocol::http2::{Http2Config, Http2KeepAlive};
/// let keep_alive = Http2KeepAlive::new(Duration::from_secs(60), Duration::from_secs(10));
/// let http2 = Http2Config::default().keep_alive(Some(keep_alive));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Http2KeepAlive {
    /// Time between keep-alive pings.
    pub interval: Duration,
    /// Time allowed for acknowledgement before closing the connection.
    pub timeout: Duration,
}

impl Http2KeepAlive {
    /// Pings after `interval` of silence and closes the connection if the
    /// acknowledgement takes longer than `timeout`.
    ///
    /// Both must be non-zero; [`Server::prepare`](crate::server::Server::prepare)
    /// refuses either at zero with
    /// [`ServerError::InvalidConfiguration`](crate::server::error::ServerError::InvalidConfiguration).
    #[must_use]
    pub const fn new(interval: Duration, timeout: Duration) -> Self {
        Self { interval, timeout }
    }
}

/// HTTP/2 tuning.
///
/// `#[non_exhaustive]`, so it grows without breaking callers — which also means
/// a struct literal will not compile outside this crate, even with `..default()`.
/// Start from [`default`](Self::default) and set what you need:
///
/// ```
/// # use kynos::server::protocol::http2::Http2Config;
/// let http2 = Http2Config::default().max_concurrent_streams(64);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Http2Config {
    /// Maximum concurrent streams on one connection.
    pub max_concurrent_streams: u32,
    /// Flow-control policy.
    pub flow_control: Http2FlowControl,
    /// Keep-alive policy. Pings after 30 seconds of silence and allows 20 for
    /// the acknowledgement by default, so a vanished peer holds its connection
    /// permit for at most 50 seconds past the last frame it sent.
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

impl Default for Http2Config {
    fn default() -> Self {
        Self {
            max_concurrent_streams: 200,
            flow_control: Http2FlowControl::Fixed {
                initial_stream_window_size: 1024 * 1024,
                initial_connection_window_size: 1024 * 1024,
            },
            keep_alive: Some(Http2KeepAlive::new(
                Duration::from_secs(30),
                Duration::from_secs(20),
            )),
            max_header_list_size: 16 * 1024,
            max_send_buffer_size: 400 * 1024,
            max_pending_accept_reset_streams: 20,
            max_local_error_reset_streams: 1024,
        }
    }
}

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

    /// Sets the keep-alive policy, or `None` to send no keep-alive pings, which
    /// leaves an idle connection open for as long as its peer's socket does.
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
