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
/// connection is never pinged. This releases a connection whose peer vanished;
/// one whose peer answers every PING is released by
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
/// `#[non_exhaustive]`, so a struct literal will not compile outside this
/// crate. Start from [`default`](Self::default) and set what you need:
///
/// ```
/// # use kynos::server::protocol::http2::Http2Config;
/// let http2 = Http2Config::default().max_concurrent_streams(64);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Http2Config {
    /// Maximum concurrent streams on one connection,
    /// [`DEFAULT_MAX_CONCURRENT_STREAMS`](Self::DEFAULT_MAX_CONCURRENT_STREAMS)
    /// by default.
    pub max_concurrent_streams: u32,
    /// Flow-control policy.
    pub flow_control: Http2FlowControl,
    /// Keep-alive policy, [`DEFAULT_KEEP_ALIVE`](Self::DEFAULT_KEEP_ALIVE) by
    /// default: a PING after 30 seconds of silence, acknowledged within 20, so
    /// a vanished peer holds its connection permit for at most 50 seconds past
    /// the last frame it sent.
    pub keep_alive: Option<Http2KeepAlive>,
    /// How long a connection may hold no stream in flight before it is sent a
    /// GOAWAY and closed. [`DEFAULT_IDLE_TIMEOUT`](Self::DEFAULT_IDLE_TIMEOUT),
    /// 30 seconds, by default, the bound HTTP/1's `header_read_timeout` puts on
    /// an idle HTTP/1 connection.
    ///
    /// A stream is in flight from its request head until its response body
    /// ends or is reset, so a long download or event stream holds the
    /// connection open however long it runs. No stream is in flight before a
    /// connection's first request head, so it also bounds that wait, counted
    /// from accept under either protocol, together with HTTP/1's
    /// `header_read_timeout`: whichever passes first closes the connection.
    ///
    /// So this and `Http1Config::header_read_timeout` share one job: how long
    /// a connection may wait for its next request. To change that bound for
    /// every client, set both to the same value. No `Server` setter does it
    /// for you, because `Server::http1` and `Server::http2` each replace their
    /// whole config, so a server-wide value would hold or vanish depending on
    /// call order.
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
            max_concurrent_streams: Self::DEFAULT_MAX_CONCURRENT_STREAMS,
            flow_control: Http2FlowControl::Fixed {
                initial_stream_window_size: 1024 * 1024,
                initial_connection_window_size: 1024 * 1024,
            },
            keep_alive: Some(Self::DEFAULT_KEEP_ALIVE),
            idle_timeout: Some(Self::DEFAULT_IDLE_TIMEOUT),
            max_header_list_size: 16 * 1024,
            max_send_buffer_size: 400 * 1024,
            max_pending_accept_reset_streams: 20,
            max_local_error_reset_streams: 1024,
        }
    }
}

impl Http2Config {
    /// The default [`max_concurrent_streams`](Self::max_concurrent_streams):
    /// 200.
    ///
    /// ```
    /// # use kynos::server::protocol::http2::Http2Config;
    /// assert_eq!(
    ///     Http2Config::default().max_concurrent_streams,
    ///     Http2Config::DEFAULT_MAX_CONCURRENT_STREAMS,
    /// );
    /// ```
    pub const DEFAULT_MAX_CONCURRENT_STREAMS: u32 = 200;

    /// The default [`keep_alive`](Self::keep_alive): a PING after 30 seconds
    /// of silence, which must be acknowledged within 20.
    ///
    /// ```
    /// # use kynos::server::protocol::http2::Http2Config;
    /// assert_eq!(
    ///     Http2Config::default().keep_alive,
    ///     Some(Http2Config::DEFAULT_KEEP_ALIVE),
    /// );
    /// ```
    pub const DEFAULT_KEEP_ALIVE: Http2KeepAlive =
        Http2KeepAlive::new(Duration::from_secs(30), Duration::from_secs(20));

    /// The default [`idle_timeout`](Self::idle_timeout): 30 seconds, the same
    /// as HTTP/1's default `header_read_timeout`.
    ///
    /// ```
    /// # use kynos::server::protocol::http2::Http2Config;
    /// assert_eq!(
    ///     Http2Config::default().idle_timeout,
    ///     Some(Http2Config::DEFAULT_IDLE_TIMEOUT),
    /// );
    /// ```
    pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

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
    /// sent a GOAWAY and closed. A connection that has not yet sent its first
    /// request head is closed once this passes from accept, even with HTTP/1's
    /// header-read timeout disabled.
    ///
    /// `None` leaves a connection whose peer answers every keep-alive PING open
    /// for as long as the peer likes.
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
    /// request head, by the idle timeout: no stream is in flight before it.
    ///
    /// `None` when the idle timeout is disabled or would overflow the clock.
    pub(in crate::server) fn first_head_deadline(
        &self,
        accepted: tokio::time::Instant,
    ) -> Option<tokio::time::Instant> {
        self.idle_timeout
            .and_then(|timeout| accepted.checked_add(timeout))
    }
}

/// The HTTP/2 streams one connection has in flight.
///
/// Two monotonic counters, so the idle wait can tell a connection that stayed
/// quiet from one that opened and finished a stream while it slept.
#[derive(Debug, Default)]
pub(in crate::server) struct Streams {
    opened: AtomicUsize,
    closed: AtomicUsize,
    /// Woken by each close that leaves none in flight.
    quiet: Notify,
}

impl Streams {
    /// Counts one stream in flight until the returned guard drops.
    pub(in crate::server) fn open(self: &Arc<Self>) -> InFlight {
        self.opened.fetch_add(1, Ordering::SeqCst);
        InFlight(Arc::clone(self))
    }

    /// Resolves once no stream has been in flight for `timeout`, and never
    /// before the first stream opens: until then the first-head deadline,
    /// which counts this same timeout from accept, is what bounds the
    /// connection.
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
