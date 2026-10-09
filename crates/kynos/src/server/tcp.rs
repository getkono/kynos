//! Options set on each accepted TCP socket.
//!
//! A peer that disappears without a FIN or RST — a mobile network change, a
//! NAT timeout, a crashed host — leaves its connection open on the server, and
//! with it a connection permit and its buffers. TCP keepalive is how the kernel
//! notices, whatever protocol the connection speaks.

use std::{net::SocketAddr, time::Duration};

use tokio::net::TcpStream;

use crate::server::error::ServerError;

/// TCP keepalive on accepted sockets.
///
/// On by default, at [`default`](Self::default). The kernel probes a
/// connection that has carried nothing for `idle`, then every `interval`, and
/// closes it after the operating system's probe count goes unanswered — nine on
/// Linux, so a vanished peer with nothing in flight is released within
/// `idle + 9 × interval`, 195 seconds by default. That bound is what releases
/// the connection permit
/// [`Server::max_connections`](crate::server::Server::max_connections) caps.
///
/// It bounds only a connection with nothing in flight: a peer that vanished
/// mid-response is released by retransmission timeouts instead, some fifteen
/// minutes at Linux's defaults. HTTP/2's
/// [`keep_alive`](crate::server::protocol::http2::Http2Config::keep_alive) bounds that
/// case for an HTTP/2 connection; nothing Kynos sets bounds it for HTTP/1.
///
/// Both durations are whole seconds on the wire, from one to Linux's ceiling of
/// 32767. The probe count is the operating system's. `interval` is applied
/// where the platform lets a socket set it — Linux, Android, the Apple
/// platforms, FreeBSD, NetBSD, illumos, Fuchsia and Windows among them — and
/// `idle` is ignored on Haiku, OpenBSD, QNX Neutrino and Vita; the operating
/// system's default applies otherwise.
///
/// `#[non_exhaustive]`, so start from [`default`](Self::default):
///
/// ```
/// # use std::time::Duration;
/// # use kynos::server::tcp::TcpKeepAlive;
/// let keepalive = TcpKeepAlive::default().idle(Duration::from_secs(120));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TcpKeepAlive {
    /// How long a connection may carry nothing before the first probe.
    pub idle: Duration,
    /// Time between unanswered probes.
    pub interval: Duration,
}

impl Default for TcpKeepAlive {
    fn default() -> Self {
        Self {
            idle: Duration::from_secs(60),
            interval: Duration::from_secs(15),
        }
    }
}

impl TcpKeepAlive {
    /// Sets how long a connection may carry nothing before the first probe.
    ///
    /// [`Server::prepare`](crate::server::Server::prepare) refuses anything
    /// outside 1 to 32767 seconds, counting whole seconds.
    #[must_use]
    pub fn idle(mut self, idle: Duration) -> Self {
        self.idle = idle;
        self
    }

    /// Sets the time between unanswered probes.
    ///
    /// [`Server::prepare`](crate::server::Server::prepare) refuses anything
    /// outside 1 to 32767 seconds, counting whole seconds.
    #[must_use]
    pub fn interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }
}

/// The longest idle time or interval Linux accepts, in seconds.
const MAX_KEEPALIVE_SECONDS: u64 = 32_767;

/// Refuses a keepalive the kernel would refuse.
///
/// `socket2` truncates to whole seconds, so half a second reaches the kernel as
/// zero, and Linux caps both at 32767. The kernel refuses either only after
/// enabling `SO_KEEPALIVE`, silently leaving the two-hour default.
pub(in crate::server) fn validate_tcp_keepalive(
    keepalive: Option<TcpKeepAlive>,
) -> std::result::Result<(), ServerError> {
    let accepted = |duration: Duration| (1..=MAX_KEEPALIVE_SECONDS).contains(&duration.as_secs());
    if keepalive.is_some_and(|keepalive| !accepted(keepalive.idle) || !accepted(keepalive.interval))
    {
        // Keeps the literal in the message in step with the constant.
        const _: () = assert!(
            MAX_KEEPALIVE_SECONDS == 32_767,
            "MAX_KEEPALIVE_SECONDS moved; the message below still says 32767"
        );
        return Err(ServerError::InvalidConfiguration(
            "TCP keepalive durations must be between 1 and 32767 seconds",
        ));
    }
    Ok(())
}

/// What the accept loop sets on every socket it accepts, built once per
/// listener rather than per connection.
#[derive(Clone, Debug)]
pub(in crate::server) struct SocketOptions {
    keepalive: Option<socket2::TcpKeepalive>,
}

impl SocketOptions {
    pub(in crate::server) fn new(keepalive: Option<TcpKeepAlive>) -> Self {
        Self {
            keepalive: keepalive.map(|keepalive| {
                let params = socket2::TcpKeepalive::new().with_time(keepalive.idle);
                // `socket2`'s own platform list for `with_interval`.
                #[cfg(any(
                    target_os = "android",
                    target_os = "dragonfly",
                    target_os = "emscripten",
                    target_os = "freebsd",
                    target_os = "fuchsia",
                    target_os = "illumos",
                    target_os = "ios",
                    target_os = "visionos",
                    target_os = "linux",
                    target_os = "macos",
                    target_os = "netbsd",
                    target_os = "tvos",
                    target_os = "watchos",
                    target_os = "windows",
                    target_os = "cygwin",
                    target_os = "nuttx",
                    all(target_os = "wasi", not(target_env = "p1")),
                ))]
                let params = params.with_interval(keepalive.interval);
                params
            }),
        }
    }

    /// Applies every option to `stream`.
    ///
    /// A failure is logged and the connection is served anyway.
    pub(in crate::server) fn apply(
        &self,
        stream: &TcpStream,
        local_addr: SocketAddr,
        peer_addr: SocketAddr,
    ) {
        if let Err(error) = stream.set_nodelay(true) {
            tracing::debug!(%error, %local_addr, %peer_addr, "could not enable TCP_NODELAY");
        }
        if let Some(keepalive) = &self.keepalive {
            if let Err(error) = socket2::SockRef::from(stream).set_tcp_keepalive(keepalive) {
                tracing::debug!(%error, %local_addr, %peer_addr, "could not enable TCP keepalive");
            }
        }
    }
}
