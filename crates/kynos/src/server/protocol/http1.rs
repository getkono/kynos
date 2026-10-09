//! HTTP/1 tuning.

use std::time::{Duration, Instant};

/// The smallest per-connection read/write buffer the crate accepts.
///
/// Both the floor `validate_protocol_config` enforces and the base the default
/// buffer is sized from.
pub(crate) const MIN_HTTP1_BUFFER_SIZE: usize = 8_192;

/// HTTP/1 tuning.
///
/// `#[non_exhaustive]`, so a struct literal will not compile outside this
/// crate. Start from [`default`](Self::default) and set what you need:
///
/// ```
/// # use kynos::server::protocol::http1::Http1Config;
/// let http1 = Http1Config::default().max_headers(64);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Http1Config {
    /// Whether to keep connections alive between requests.
    pub keep_alive: bool,
    /// How long a client may take to send the request head.
    ///
    /// The first request head on any connection is held to it from accept,
    /// whatever protocol the connection turns out to speak, over TLS too.
    /// Past the first head, it bounds each later HTTP/1 head; an
    /// HTTP/2 connection is then held to the HTTP/2 idle timeout instead.
    pub header_read_timeout: Option<Duration>,
    /// The maximum number of request headers.
    pub max_headers: usize,
    /// The maximum per-connection read/write buffer size.
    pub max_buffer_size: usize,
}

impl Default for Http1Config {
    fn default() -> Self {
        Self {
            keep_alive: true,
            header_read_timeout: Some(Duration::from_secs(30)),
            max_headers: 100,
            max_buffer_size: MIN_HTTP1_BUFFER_SIZE + 4_096 * 100,
        }
    }
}

impl Http1Config {
    /// Sets whether to keep connections alive between requests.
    #[must_use]
    pub fn keep_alive(mut self, keep_alive: bool) -> Self {
        self.keep_alive = keep_alive;
        self
    }

    /// Sets how long a client may take to send the request head, counted from
    /// accept for a connection's first head under either protocol.
    ///
    /// `None` waits indefinitely: a client that never finishes a request head
    /// holds the connection open.
    ///
    /// [`Server::prepare`](crate::server::Server::prepare) refuses
    /// `Some(Duration::ZERO)`.
    #[must_use]
    pub fn header_read_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.header_read_timeout = timeout;
        self
    }

    /// Sets the maximum number of request headers.
    ///
    /// [`Server::prepare`](crate::server::Server::prepare) refuses zero.
    #[must_use]
    pub fn max_headers(mut self, max_headers: usize) -> Self {
        self.max_headers = max_headers;
        self
    }

    /// Sets the maximum per-connection read/write buffer size.
    ///
    /// [`Server::prepare`](crate::server::Server::prepare) refuses anything
    /// below 8192 bytes.
    #[must_use]
    pub fn max_buffer_size(mut self, max_buffer_size: usize) -> Self {
        self.max_buffer_size = max_buffer_size;
        self
    }
}

impl Http1Config {
    /// When a connection accepted at `accepted` must have produced its first
    /// request head, under either protocol.
    ///
    /// hyper's header-read timer starts only once an HTTP/1 codec runs. `None`
    /// when the timeout is disabled or would overflow the clock.
    pub(in crate::server) fn first_head_deadline(&self, accepted: Instant) -> Option<Instant> {
        self.header_read_timeout
            .and_then(|timeout| accepted.checked_add(timeout))
    }
}

/// The HTTP/1 header cap the driver is told about.
///
/// A function so the decision can be asserted without a socket.
pub(in crate::server) const fn forwarded_max_headers(config: &Http1Config) -> usize {
    config.max_headers
}
