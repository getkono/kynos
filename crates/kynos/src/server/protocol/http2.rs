//! HTTP/2 tuning, and the in-flight stream count its idle timeout is read
//! against.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use tokio::sync::Notify;

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
/// whose peer vanished with no stream open. A peer that answers every PING is
/// alive by this measure, so what releases it is
/// [`Http2Config::idle_timeout`] instead.
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
    /// How long a connection may hold no stream in flight before it is sent a
    /// GOAWAY and closed. 30 seconds by default, the bound HTTP/1's
    /// `header_read_timeout` puts on an idle HTTP/1 connection.
    ///
    /// A stream is in flight from its request head until its response body
    /// ends or is reset, so a long download or event stream holds the
    /// connection open however long it runs. In a build without `http1`, it
    /// also bounds the wait for a connection's first request head, counted
    /// from accept.
    pub idle_timeout: Option<Duration>,
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
            idle_timeout: Some(Duration::from_secs(30)),
            max_header_list_size: 16 * 1024,
            max_send_buffer_size: 400 * 1024,
            max_pending_accept_reset_streams: 20,
            max_local_error_reset_streams: 1024,
        }
    }
}

impl Http2Config {
    /// Sets the maximum concurrent streams on one connection.
    ///
    /// [`Server::prepare`](crate::server::Server::prepare) refuses zero.
    #[must_use]
    pub fn max_concurrent_streams(mut self, streams: u32) -> Self {
        self.max_concurrent_streams = streams;
        self
    }

    /// Sets the flow-control policy.
    ///
    /// [`Server::prepare`](crate::server::Server::prepare) refuses a
    /// [`Fixed`](Http2FlowControl::Fixed) policy with either window zero or
    /// above 2<sup>31</sup>-1 octets, the largest window RFC 9113 §6.5.2 allows.
    #[must_use]
    pub fn flow_control(mut self, flow_control: Http2FlowControl) -> Self {
        self.flow_control = flow_control;
        self
    }

    /// Sets the keep-alive policy, or `None` to send no keep-alive pings, which
    /// leaves an idle connection open for as long as its peer's socket does.
    ///
    /// [`Server::prepare`](crate::server::Server::prepare) refuses a policy
    /// whose `interval` or `timeout` is zero.
    #[must_use]
    pub fn keep_alive(mut self, keep_alive: Option<Http2KeepAlive>) -> Self {
        self.keep_alive = keep_alive;
        self
    }

    /// Sets how long a connection may hold no stream in flight before it is
    /// sent a GOAWAY and closed.
    ///
    /// `None` leaves a connection whose peer answers every keep-alive PING open
    /// for as long as the peer likes, which is a decision rather than a default.
    ///
    /// [`Server::prepare`](crate::server::Server::prepare) refuses
    /// `Some(Duration::ZERO)`.
    #[must_use]
    pub fn idle_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Sets the maximum decoded request header-list size.
    ///
    /// [`Server::prepare`](crate::server::Server::prepare) refuses zero.
    #[must_use]
    pub fn max_header_list_size(mut self, size: u32) -> Self {
        self.max_header_list_size = size;
        self
    }

    /// Sets the maximum buffered response bytes per stream.
    ///
    /// [`Server::prepare`](crate::server::Server::prepare) refuses zero, and
    /// anything above `u32::MAX`.
    #[must_use]
    pub fn max_send_buffer_size(mut self, size: usize) -> Self {
        self.max_send_buffer_size = size;
        self
    }

    /// Sets the maximum peer-created reset streams awaiting acceptance.
    ///
    /// [`Server::prepare`](crate::server::Server::prepare) refuses zero.
    #[must_use]
    pub fn max_pending_accept_reset_streams(mut self, streams: usize) -> Self {
        self.max_pending_accept_reset_streams = streams;
        self
    }

    /// Sets the maximum locally reset streams retained before sending GOAWAY.
    ///
    /// [`Server::prepare`](crate::server::Server::prepare) refuses zero.
    #[must_use]
    pub fn max_local_error_reset_streams(mut self, streams: usize) -> Self {
        self.max_local_error_reset_streams = streams;
        self
    }
}

impl Http2Config {
    /// When a connection accepted at `accepted` must have produced its first
    /// request head, in a build with no HTTP/1 header-read timeout to count.
    ///
    /// `None` when the idle timeout is disabled or would overflow the clock.
    #[cfg(not(feature = "http1"))]
    pub(in crate::server) fn first_head_deadline(
        &self,
        accepted: std::time::Instant,
    ) -> Option<std::time::Instant> {
        self.idle_timeout
            .and_then(|timeout| accepted.checked_add(timeout))
    }
}

/// The HTTP/2 streams one connection has in flight.
///
/// Two monotonic counters rather than one that rises and falls, so the idle
/// wait can tell a connection that stayed quiet from one that opened and
/// finished a stream while it slept: both end with nothing in flight, and only
/// the first is idle.
#[derive(Debug, Default)]
pub(in crate::server) struct Streams {
    opened: AtomicUsize,
    closed: AtomicUsize,
    /// Woken by the first stream, and by each close that leaves none in flight.
    quiet: Notify,
}

impl Streams {
    /// Counts one stream in flight until the returned guard drops.
    pub(in crate::server) fn open(self: &Arc<Self>) -> InFlight {
        if self.opened.fetch_add(1, Ordering::SeqCst) == 0 {
            self.quiet.notify_one();
        }
        InFlight(Arc::clone(self))
    }

    /// Resolves once no stream has been in flight for `timeout`, and never
    /// before the first stream opens: until then the first-head deadline is
    /// what bounds the connection.
    pub(in crate::server) async fn idle(&self, timeout: Option<Duration>) {
        let Some(timeout) = timeout else {
            return std::future::pending().await;
        };
        loop {
            let quiet = self.quiet.notified();
            tokio::pin!(quiet);
            // Registered before the counters are read, so a close between the
            // reads and the wait is not missed.
            quiet.as_mut().enable();
            // `closed` first: it never passes `opened`, so equal readings mean
            // nothing was in flight when `closed` was read.
            let closed = self.closed.load(Ordering::SeqCst);
            let opened = self.opened.load(Ordering::SeqCst);
            if opened == 0 || closed != opened {
                quiet.await;
                continue;
            }
            // A stream that opens and closes during the sleep wakes `quiet`,
            // which restarts the idle period from that close.
            tokio::select! {
                () = tokio::time::sleep(timeout) => {}
                () = &mut quiet => continue,
            }
            // One still in flight wakes nothing until it closes.
            if self.opened.load(Ordering::SeqCst) == opened {
                return;
            }
        }
    }
}

/// One stream counted in flight by [`Streams`].
#[derive(Debug)]
pub(in crate::server) struct InFlight(Arc<Streams>);

impl Drop for InFlight {
    fn drop(&mut self) {
        let streams = &self.0;
        let closed = streams
            .closed
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1);
        if closed == streams.opened.load(Ordering::SeqCst) {
            streams.quiet.notify_one();
        }
    }
}
