#[cfg(feature = "http1")]
use crate::server::protocol::http1::Http1Config;
#[cfg(feature = "http2")]
use crate::server::protocol::http2::{Http2Config, Http2FlowControl, Http2KeepAlive};

/// The whole struct, so a field added or changed without a decision here
/// fails rather than going unchecked.
#[cfg(feature = "http1")]
#[test]
fn http1_defaults_are_owned_by_kynos() {
    assert_eq!(
        Http1Config::default(),
        Http1Config {
            keep_alive: true,
            header_read_timeout: Some(std::time::Duration::from_secs(30)),
            max_headers: 100,
            max_buffer_size: 417_792,
        }
    );
}

/// The whole struct, for the reason the HTTP/1 case gives.
#[cfg(feature = "http2")]
#[test]
fn http2_defaults_are_owned_by_kynos() {
    assert_eq!(
        Http2Config::default(),
        Http2Config {
            max_concurrent_streams: 200,
            flow_control: Http2FlowControl::Fixed {
                initial_stream_window_size: 1024 * 1024,
                initial_connection_window_size: 1024 * 1024,
            },
            keep_alive: Some(Http2KeepAlive {
                interval: std::time::Duration::from_secs(30),
                timeout: std::time::Duration::from_secs(20),
            }),
            idle_timeout: Some(std::time::Duration::from_secs(30)),
            max_header_list_size: 16 * 1024,
            max_send_buffer_size: 400 * 1024,
            max_pending_accept_reset_streams: 20,
            max_local_error_reset_streams: 1024,
        }
    );
}

/// Every setter writes its own field and no other: each value differs from
/// its default, and the whole struct is compared.
#[cfg(feature = "http1")]
#[test]
fn http1_setters_write_their_own_fields() {
    use std::time::Duration;

    let http1 = Http1Config::default()
        .keep_alive(false)
        .header_read_timeout(Some(Duration::from_secs(5)))
        .max_headers(64)
        .max_buffer_size(65_536);
    assert_eq!(
        http1,
        Http1Config {
            keep_alive: false,
            header_read_timeout: Some(Duration::from_secs(5)),
            max_headers: 64,
            max_buffer_size: 65_536,
        }
    );
}

/// Every setter writes its own field and no other: each value differs from
/// its default, and the whole struct is compared.
#[cfg(feature = "http2")]
#[test]
fn http2_setters_write_their_own_fields() {
    use std::time::Duration;

    let keep_alive = Http2KeepAlive::new(Duration::from_secs(7), Duration::from_secs(3));
    let http2 = Http2Config::default()
        .max_concurrent_streams(64)
        .flow_control(Http2FlowControl::Adaptive)
        .keep_alive(Some(keep_alive))
        .idle_timeout(Some(Duration::from_secs(9)))
        .max_header_list_size(8 * 1024)
        .max_send_buffer_size(128 * 1024)
        .max_pending_accept_reset_streams(5)
        .max_local_error_reset_streams(256);
    assert_eq!(
        http2,
        Http2Config {
            max_concurrent_streams: 64,
            flow_control: Http2FlowControl::Adaptive,
            keep_alive: Some(keep_alive),
            idle_timeout: Some(Duration::from_secs(9)),
            max_header_list_size: 8 * 1024,
            max_send_buffer_size: 128 * 1024,
            max_pending_accept_reset_streams: 5,
            max_local_error_reset_streams: 256,
        }
    );
}

/// The whole retry schedule: the wait doubles from 10 ms to a one-second cap,
/// and a listener never gives up on a failure waiting can clear.
#[test]
fn a_failing_accept_backs_off_by_doubling_to_a_cap_and_never_gives_up() {
    use std::{io, time::Duration};

    use crate::server::accept::{AcceptBackoff, AcceptRetry};

    let error = io::Error::other("accept failed");
    let mut backoff = AcceptBackoff::default();
    let schedule = (0..10).map(|_| backoff.fail(&error)).collect::<Vec<_>>();

    assert_eq!(
        schedule,
        [10, 20, 40, 80, 160, 320, 640, 1000, 1000, 1000]
            .map(|millis| AcceptRetry::After(Duration::from_millis(millis)))
    );
}

/// Running out of file descriptors, for the process (EMFILE) or the system
/// (ENFILE), is not a listener failure: a client holding enough idle
/// connections open must not stop the server. Both numbers are the same on
/// every Unix.
#[cfg(unix)]
#[test]
fn running_out_of_file_descriptors_never_ends_the_listener() {
    use std::{io, time::Duration};

    use crate::server::accept::{AcceptBackoff, AcceptRetry};

    for (name, errno) in [("EMFILE", 24), ("ENFILE", 23)] {
        let error = io::Error::from_raw_os_error(errno);
        let mut backoff = AcceptBackoff::default();
        let last = (0..100).map(|_| backoff.fail(&error)).last();

        assert_eq!(
            last,
            Some(AcceptRetry::After(Duration::from_secs(1))),
            "{name}"
        );
    }
}

/// A listener that is no longer listening cannot be waited back into
/// service, so the first such failure ends it.
#[test]
fn a_listener_that_is_not_listening_ends_at_once() {
    use std::io;

    use crate::server::accept::{AcceptBackoff, AcceptRetry};

    let error = io::Error::from(io::ErrorKind::InvalidInput);

    assert_eq!(AcceptBackoff::default().fail(&error), AcceptRetry::Never);
}

/// A failure that belonged to one queued connection retries at once and does
/// not advance the schedule.
#[test]
fn a_failure_of_one_queued_connection_retries_at_once() {
    use std::{io, time::Duration};

    use crate::server::accept::{AcceptBackoff, AcceptRetry};

    let mut backoff = AcceptBackoff::default();
    for kind in [
        io::ErrorKind::Interrupted,
        io::ErrorKind::ConnectionAborted,
        io::ErrorKind::ConnectionReset,
    ] {
        assert_eq!(
            backoff.fail(&io::Error::from(kind)),
            AcceptRetry::Now,
            "{kind:?}"
        );
    }

    assert_eq!(
        backoff.fail(&io::Error::other("accept failed")),
        AcceptRetry::After(Duration::from_millis(10))
    );
}

/// Linux's accept(2) reports a network error of the dequeued connection and
/// says to retry it like EAGAIN, so it retries at once.
#[cfg(any(target_os = "linux", target_os = "android"))]
#[test]
fn a_network_error_of_one_queued_connection_retries_at_once() {
    use std::{io, time::Duration};

    use crate::server::accept::{AcceptBackoff, AcceptRetry};

    let mut backoff = AcceptBackoff::default();
    for kind in [
        io::ErrorKind::NetworkDown,
        io::ErrorKind::NetworkUnreachable,
        io::ErrorKind::HostUnreachable,
    ] {
        assert_eq!(
            backoff.fail(&io::Error::from(kind)),
            AcceptRetry::Now,
            "{kind:?}"
        );
    }

    assert_eq!(
        backoff.fail(&io::Error::other("accept failed")),
        AcceptRetry::After(Duration::from_millis(10))
    );
}

/// Elsewhere a network error from accept is the listener's: Windows reports
/// `WSAENETDOWN` when the network subsystem has failed. Retrying it at once
/// would spin the accept loop, so it waits like any other failure.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
#[test]
fn a_network_error_of_the_listener_waits() {
    use std::{io, time::Duration};

    use crate::server::accept::{AcceptBackoff, AcceptRetry};

    let mut backoff = AcceptBackoff::default();
    for (kind, millis) in [
        (io::ErrorKind::NetworkDown, 10),
        (io::ErrorKind::NetworkUnreachable, 20),
        (io::ErrorKind::HostUnreachable, 40),
    ] {
        assert_eq!(
            backoff.fail(&io::Error::from(kind)),
            AcceptRetry::After(Duration::from_millis(millis)),
            "{kind:?}"
        );
    }
}

/// A successful accept starts the schedule over, so the next failure waits
/// 10 ms again.
#[test]
fn a_successful_accept_restarts_the_backoff() {
    use std::{io, time::Duration};

    use crate::server::accept::{AcceptBackoff, AcceptRetry};

    let error = io::Error::other("accept failed");
    let mut backoff = AcceptBackoff::default();
    backoff.fail(&error);
    backoff.fail(&error);
    backoff.succeed();

    assert_eq!(
        backoff.fail(&error),
        AcceptRetry::After(Duration::from_millis(10))
    );
}

#[test]
fn tcp_keepalive_defaults_are_owned_by_kynos() {
    use std::time::Duration;

    use crate::server::tcp::TcpKeepAlive;

    let keepalive = TcpKeepAlive::default();
    assert_eq!(keepalive.idle, Duration::from_secs(60));
    assert_eq!(keepalive.interval, Duration::from_secs(15));
    assert_eq!(
        crate::server::Server::new(test_service()).tcp_keepalive,
        Some(keepalive),
        "a server keeps accepted sockets alive unless told not to"
    );
}

/// A keepalive the kernel would refuse is refused before a socket is bound.
///
/// Both ends matter for the same reason. Half a second reaches the socket
/// option as zero, and an hour past Linux's 32767-second ceiling is over it;
/// either way Linux rejects the time only after enabling `SO_KEEPALIVE`, which
/// leaves the socket probing at the system's two-hour default instead.
#[test]
fn a_tcp_keepalive_the_kernel_would_refuse_is_refused() {
    use std::time::Duration;

    use crate::server::{
        error::ServerError,
        tcp::{TcpKeepAlive, validate_tcp_keepalive},
    };

    for keepalive in [
        TcpKeepAlive::default().idle(Duration::ZERO),
        TcpKeepAlive::default().interval(Duration::ZERO),
        TcpKeepAlive::default().idle(Duration::from_millis(500)),
        TcpKeepAlive::default().interval(Duration::from_millis(999)),
        TcpKeepAlive::default().idle(Duration::from_secs(32_768)),
        TcpKeepAlive::default().interval(Duration::from_secs(32_768)),
    ] {
        assert!(
            matches!(
                validate_tcp_keepalive(Some(keepalive)),
                Err(ServerError::InvalidConfiguration(
                    "TCP keepalive durations must be between 1 and 32767 seconds"
                ))
            ),
            "{keepalive:?} must be refused"
        );
    }
    validate_tcp_keepalive(None).expect("no keepalive is a configuration");
    for bound in [1, 32_767] {
        validate_tcp_keepalive(Some(
            TcpKeepAlive::default()
                .idle(Duration::from_secs(bound))
                .interval(Duration::from_secs(bound)),
        ))
        .expect("both bounds are accepted");
    }
}

/// `prepare` refuses what the validator refuses, rather than binding with it.
#[tokio::test]
async fn prepare_refuses_a_tcp_keepalive_the_kernel_would_refuse() {
    use std::time::Duration;

    use crate::server::tcp::TcpKeepAlive;

    let error = crate::server::Server::new(test_service())
        .tcp_keepalive(Some(TcpKeepAlive::default().idle(Duration::ZERO)))
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .prepare()
        .await
        .expect_err("a refused keepalive prevents preparation");
    assert!(matches!(
        error,
        crate::Error::Server(crate::server::error::ServerError::InvalidConfiguration(
            "TCP keepalive durations must be between 1 and 32767 seconds"
        ))
    ));
}

/// Accepts one loopback connection and applies `options` to it, as the accept
/// loop does, returning the server's side for inspection.
async fn accepted_with(
    options: &crate::server::tcp::SocketOptions,
) -> (tokio::net::TcpStream, tokio::net::TcpStream) {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("loopback listener binds");
    let local_addr = listener.local_addr().expect("listener has an address");
    let client = tokio::net::TcpStream::connect(local_addr)
        .await
        .expect("client connects");
    let (accepted, peer_addr) = listener.accept().await.expect("server accepts");
    options.apply(&accepted, local_addr, peer_addr);
    (accepted, client)
}

/// What the accept loop sets is what the kernel holds for the socket.
///
/// Read back through the socket rather than through Kynos's own value, because
/// the value is not the claim: a keepalive computed and never set would satisfy
/// an assertion on it. Linux only, because reading the idle time and interval
/// back is not portable, and CI's runner is Linux.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn an_accepted_socket_carries_the_configured_keepalive() {
    use std::time::Duration;

    use crate::server::tcp::{SocketOptions, TcpKeepAlive};

    let keepalive = TcpKeepAlive::default()
        .idle(Duration::from_secs(42))
        .interval(Duration::from_secs(7));
    let (accepted, _client) = accepted_with(&SocketOptions::new(Some(keepalive))).await;
    let socket = socket2::SockRef::from(&accepted);

    assert!(socket.keepalive().expect("SO_KEEPALIVE reads"));
    assert_eq!(
        socket.tcp_keepalive_time().expect("TCP_KEEPIDLE reads"),
        Duration::from_secs(42)
    );
    assert_eq!(
        socket
            .tcp_keepalive_interval()
            .expect("TCP_KEEPINTVL reads"),
        Duration::from_secs(7)
    );
    assert!(accepted.nodelay().expect("TCP_NODELAY reads"));
}

/// The kernel's timer for the server's end of a loopback connection, read from
/// `/proc/net/tcp`: `2` is the keepalive timer, and it runs only on a socket
/// with `SO_KEEPALIVE` set and nothing awaiting acknowledgement.
#[cfg(all(target_os = "linux", feature = "http1"))]
fn server_side_timer(server: std::net::SocketAddr, client: std::net::SocketAddr) -> Option<u8> {
    fn hex((ip, port): (std::net::Ipv4Addr, u16)) -> String {
        // The kernel prints the network-order address as a native word, so the
        // octets are read back in this host's byte order.
        format!("{:08X}:{port:04X}", u32::from_ne_bytes(ip.octets()))
    }
    let v4 = |address: std::net::SocketAddr| match address {
        std::net::SocketAddr::V4(address) => (*address.ip(), address.port()),
        std::net::SocketAddr::V6(_) => unreachable!("the test binds IPv4 loopback"),
    };
    let (local, remote) = (hex(v4(server)), hex(v4(client)));

    std::fs::read_to_string("/proc/net/tcp")
        .expect("/proc/net/tcp reads")
        .lines()
        .skip(1)
        .find_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            (fields[1] == local && fields[2] == remote)
                .then(|| fields[5].split(':').next()?.parse().ok())
                .flatten()
        })
}

/// What `Server::tcp_keepalive` configures reaches the socket the accept loop
/// accepted, through `prepare` and the loop rather than beside them.
///
/// The test above proves `SocketOptions::apply` sets what it is given; this one
/// is what fails if the loop stops calling it or `prepare` stops handing it the
/// configured value. The socket the server holds is not reachable from a test,
/// so the kernel is asked instead: its keepalive timer runs on the server's end
/// of the connection exactly when `SO_KEEPALIVE` is set there. Linux only, for
/// `/proc/net/tcp`.
#[cfg(all(target_os = "linux", feature = "http1"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_accept_loop_sets_the_configured_keepalive_on_what_it_accepts() {
    use std::time::Duration;

    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    use crate::server::tcp::TcpKeepAlive;

    for (keepalive, expected) in [(Some(TcpKeepAlive::default()), 2), (None, 0)] {
        let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
        let bound = crate::server::Server::new(test_service())
            .tcp_keepalive(keepalive)
            .bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
                let _ = shutdown_receiver.await;
            }))
            .prepare()
            .await
            .expect("loopback listener binds");
        let address = bound.local_addrs()[0];
        let server = tokio::spawn(bound.serve());

        // One exchange, so the server has certainly accepted and configured the
        // socket before the kernel is asked about it.
        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("server accepts");
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .expect("request writes");
        let mut response = [0_u8; 64];
        let read = stream.read(&mut response).await.expect("response reads");
        assert!(response[..read].starts_with(b"HTTP/1.1 200"));

        let client = stream.local_addr().expect("client has an address");
        // The response's retransmission timer holds the slot until the client's
        // acknowledgement lands, so wait for the socket to settle.
        let timer = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match server_side_timer(address, client) {
                    Some(timer) if timer != 1 => return timer,
                    _ => tokio::time::sleep(Duration::from_millis(10)).await,
                }
            }
        })
        .await
        .expect("the server's socket settles");
        assert_eq!(timer, expected, "keepalive {keepalive:?}");

        drop(stream);
        let _ = shutdown_sender.send(());
        server
            .await
            .expect("server task joins")
            .expect("server exits cleanly");
    }
}

#[tokio::test]
async fn no_keepalive_leaves_an_accepted_socket_without_one() {
    use crate::server::tcp::SocketOptions;

    let (accepted, _client) = accepted_with(&SocketOptions::new(None)).await;

    assert!(
        !socket2::SockRef::from(&accepted)
            .keepalive()
            .expect("SO_KEEPALIVE reads")
    );
    assert!(accepted.nodelay().expect("TCP_NODELAY reads"));
}

/// `accept.rs` clones the whole `TransportConfig` per accepted socket, so this
/// is a per-connection cost rather than a per-server one. Measured at 40 bytes
/// and rounded up to the next multiple of 64, since `docs/nfr.md#thresholds`
/// asks for a recorded measurement rather than a chosen number.
#[cfg(feature = "http1")]
#[test]
fn an_http1_config_is_cheap_to_copy_per_connection() {
    let http1 = size_of::<Http1Config>();

    assert!(
        http1 <= 64,
        "Http1Config grew to {http1} bytes from a measured 40; \
         it is copied once per accepted socket"
    );
}

/// The same, for the HTTP/2 half. Measured at 96 bytes, rounded up to 128.
///
/// `Http2FlowControl` and `Http2KeepAlive` get no ceiling of their own because
/// neither is ever held per connection on its own. This bound does not
/// substitute for one: 96 against 128 leaves 32 bytes of slack, so either could
/// grow by its own `Duration` before it fires.
#[cfg(feature = "http2")]
#[test]
fn an_http2_config_is_cheap_to_copy_per_connection() {
    let http2 = size_of::<Http2Config>();

    assert!(
        http2 <= 128,
        "Http2Config grew to {http2} bytes from a measured 96; \
         it is copied once per accepted socket"
    );
}

/// `TransportConfig` is the struct `accept.rs` actually clones per socket, which
/// is what makes the two ceilings above per-connection costs at all.
///
/// Measured at 200 bytes with every feature on, which is where it is widest --
/// it gains its TLS runtime there -- and rounded up to 256, so the ceiling holds
/// at every smaller feature set by construction. Ungated for that reason.
///
/// 256 is well under the smallest read/write buffer the configuration
/// configures, so describing a connection never costs more than serving one.
/// That relation is prose rather than an assertion: nothing can falsify it while
/// this ceiling holds, and `MIN_HTTP1_BUFFER_SIZE` is pinned by a `const`
/// assertion in `protocol.rs`. `docs/architecture.md` records it in
/// "Why hyper stays".
#[test]
fn a_transport_config_is_cheap_to_clone_per_connection() {
    let config = size_of::<super::TransportConfig>();

    assert!(
        config <= 256,
        "TransportConfig grew to {config} bytes from a measured 200; \
         it is cloned once per accepted socket"
    );
}

/// What `Server::new` starts from, read off the server it builds rather than
/// off the constants it was meant to read: the drain deadline its setter
/// documents, leaving a margin under a 30-second termination window, and the
/// connection cap `max_connections` documents.
#[test]
fn a_new_server_starts_from_its_documented_limits() {
    let server = crate::server::Server::new(test_service());

    assert_eq!(server.shutdown_timeout, std::time::Duration::from_secs(25));
    assert_eq!(server.max_connections.get(), 10_000);
}

#[tokio::test]
async fn prepare_requires_a_listener() {
    let service = test_service();
    let error = crate::server::Server::new(service)
        .prepare()
        .await
        .expect_err("a listener is required");
    assert!(matches!(
        error,
        crate::Error::Server(crate::server::error::ServerError::NoListeners)
    ));
}

#[tokio::test]
async fn prepare_exposes_operating_system_selected_ports() {
    let bound = crate::server::Server::new(test_service())
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .prepare()
        .await
        .expect("loopback listener binds");
    assert_eq!(bound.local_addrs().len(), 1);
    assert_ne!(bound.local_addrs()[0].port(), 0);
}

#[tokio::test]
async fn prepare_accepts_a_standard_library_listener() {
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("standard listener binds");
    let expected = listener.local_addr().expect("listener has an address");
    let bound = crate::server::Server::new(test_service())
        .listener(listener)
        .prepare()
        .await
        .expect("standard listener converts to Tokio ownership");
    assert_eq!(bound.local_addrs(), [expected]);
}

/// A failed bind names the address that failed, and lets go of every listener
/// prepared before it: a supplied one and one bound from an earlier address
/// can both be bound again at once.
///
/// The earlier address is a port reserved and freed just before, since only
/// a known port can be checked for release.
#[tokio::test]
async fn binding_is_atomic_when_a_later_address_is_unavailable() {
    let local = || std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0));
    let supplied = local().expect("a supplied listener binds");
    let supplied_address = supplied.local_addr().expect("listener has an address");
    let earlier_address = local()
        .and_then(|reserved| reserved.local_addr())
        .expect("a port is reserved and freed");
    let occupied = local().expect("port is reserved");
    let occupied_address = occupied.local_addr().expect("listener has an address");

    let error = crate::server::Server::new(test_service())
        .listener(supplied)
        .bind(earlier_address)
        .bind(occupied_address)
        .prepare()
        .await
        .expect_err("the occupied address prevents preparation");

    assert!(
        matches!(
            &error,
            crate::Error::Server(crate::server::error::ServerError::Bind { address, .. })
                if *address == occupied_address
        ),
        "{error:?}"
    );
    for released in [supplied_address, earlier_address] {
        drop(
            std::net::TcpListener::bind(released)
                .unwrap_or_else(|error| panic!("{released} is still held: {error}")),
        );
    }
}

/// A configured name is resolved when the server is prepared, and every
/// address it resolves to gets a listener of its own, in the resolver's
/// order. `localhost` is the one name every host resolves without asking a
/// network, often to both loopback addresses.
///
/// A host may list `::1` for `localhost` while its IPv6 loopback is disabled.
/// Binding is atomic, so there the expected answer is the refusal of the
/// first address the host cannot bind, which a probe bind finds beforehand.
#[tokio::test]
async fn a_name_is_bound_once_per_address_it_resolves_to() {
    let resolved = tokio::net::lookup_host("localhost:0")
        .await
        .expect("localhost resolves")
        .map(|address| address.ip())
        .collect::<Vec<_>>();
    let unbindable = resolved
        .iter()
        .map(|ip| std::net::SocketAddr::new(*ip, 0))
        .find(|address| std::net::TcpListener::bind(address).is_err());

    for configured in [
        crate::server::address::BindAddress::from("localhost:0"),
        crate::server::address::BindAddress::from(("localhost", 0)),
    ] {
        let prepared = crate::server::Server::new(test_service())
            .bind(configured.clone())
            .prepare()
            .await;
        if let Some(unbindable) = unbindable {
            let error = prepared.expect_err("an address the host cannot bind is refused");
            assert!(
                matches!(
                    &error,
                    crate::Error::Server(crate::server::error::ServerError::Bind { address, .. })
                        if *address == unbindable
                ),
                "{configured}: {error:?}"
            );
            continue;
        }
        let bound = prepared.unwrap_or_else(|error| panic!("{configured} binds: {error:?}"));
        let addresses = bound.local_addrs();

        assert_eq!(
            addresses
                .iter()
                .map(std::net::SocketAddr::ip)
                .collect::<Vec<_>>(),
            resolved,
            "{configured}"
        );
        assert!(
            addresses.iter().all(|address| address.port() != 0),
            "{configured}: {addresses:?}"
        );
    }
}

/// A name that does not resolve is refused as itself, before anything binds,
/// in either form a name takes: one string, or a host and a port. Both are
/// refused before any resolver is asked, so the outcome does not depend on
/// the host's DNS: the first has no port to parse, and the second holds a NUL
/// byte, which no C resolver call can be handed.
#[tokio::test]
async fn a_name_that_does_not_resolve_is_refused_as_itself() {
    for configured in [
        crate::server::address::BindAddress::from("localhost"),
        crate::server::address::BindAddress::from(("kynos\0invalid", 80)),
    ] {
        let error = crate::server::Server::new(test_service())
            .bind(configured.clone())
            .prepare()
            .await
            .expect_err("an unresolvable name prevents preparation");

        assert!(
            matches!(
                &error,
                crate::Error::Server(crate::server::error::ServerError::Resolve { address, .. })
                    if *address == configured
            ),
            "{configured}: {error:?}"
        );
    }
}

#[cfg(feature = "http1")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http1_serves_and_shuts_down_over_a_real_socket() {
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(test_service())
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());

    let response = tokio::task::spawn_blocking(move || request_http1(address))
        .await
        .expect("blocking client joins");

    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.ends_with("ok"));
    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// One configured server serves two connections from one built service while
/// the caller holds a reference of its own: each connection shares the
/// service while it runs and releases it when it ends, so the last reference
/// drops where the caller drops it, never inside a connection's final poll.
#[cfg(feature = "http1")]
#[tokio::test]
async fn one_server_serves_two_connections_from_a_service_the_caller_shares() {
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let server = crate::server::Server::new(test_service());
    let held = Arc::clone(&server.service);
    let peer = "192.0.2.1:50000".parse().expect("a socket address");

    let mut clients = Vec::new();
    let mut connections = Vec::new();
    for _ in 0..2 {
        let (client, io) = tokio::io::duplex(4096);
        clients.push(client);
        connections.push(tokio::spawn(
            server
                .serve_io(io, peer)
                .expect("the default configuration is accepted"),
        ));
    }
    assert_eq!(
        Arc::strong_count(&held),
        4,
        "the server, the caller and each running connection hold the service"
    );

    for client in &mut clients {
        client
            .write_all(b"GET / HTTP/1.1\r\nhost: example\r\nconnection: close\r\n\r\n")
            .await
            .expect("the request is written");
    }
    for client in &mut clients {
        let mut response = String::new();
        client
            .read_to_string(&mut response)
            .await
            .expect("the response is read");
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(response.ends_with("ok"), "{response}");
    }
    for connection in connections {
        connection
            .await
            .expect("the connection task joins")
            .expect("the connection ends cleanly");
    }

    assert_eq!(
        Arc::strong_count(&held),
        2,
        "a finished connection releases its reference"
    );
    drop(server);
    assert_eq!(
        Arc::strong_count(&held),
        1,
        "the caller's reference outlives the server and every connection"
    );
}

#[cfg(feature = "http1")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_closes_listeners_while_an_http1_request_drains() {
    use http_body_util::{BodyExt as _, Empty};
    use hyper_util::rt::TokioIo;

    let (service, started, release) = blocking_service();
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(service)
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("HTTP/1 handshake completes");
    let connection = tokio::spawn(connection);
    let request = tokio::spawn(async move {
        let request = hyper::Request::builder()
            .uri("/")
            .header(hyper::header::HOST, "localhost")
            .body(Empty::<bytes::Bytes>::new())
            .expect("request builds");
        sender
            .send_request(request)
            .await
            .expect("request succeeds")
            .into_body()
            .collect()
            .await
            .expect("response body reads")
            .to_bytes()
    });

    tokio::time::timeout(std::time::Duration::from_secs(1), started.notified())
        .await
        .expect("the request reaches the handler");
    let _ = shutdown_sender.send(());

    // Windows answers a reset by retrying the SYN, so a connect to a closed
    // port is refused only after about two seconds where Linux and macOS
    // refuse it at once. Each attempt is bounded past that, or none would ever
    // observe the refusal there.
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                tokio::net::TcpStream::connect(address),
            )
            .await
            {
                Ok(Err(_)) => break,
                Ok(Ok(stream)) => drop(stream),
                Err(_) => {}
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the listener closes while the request drains");
    assert!(
        !server.is_finished(),
        "the active request must keep draining"
    );

    release.notify_one();
    assert_eq!(
        request.await.expect("request task joins"),
        bytes::Bytes::from_static(b"ok")
    );
    connection
        .await
        .expect("client connection task joins")
        .expect("client connection closes cleanly");
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

#[cfg(feature = "http1")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connection_limit_applies_before_accepting_another_socket() {
    use std::{
        num::NonZeroUsize,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    let calls = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(tokio::sync::Notify::new());
    let service: crate::router::service::Service<()> = {
        let calls = Arc::clone(&calls);
        let release = Arc::clone(&release);
        let document = kynos_openapi::Document::new(
            kynos_openapi::SpecVersion::V3_1,
            kynos_openapi::Info::new("Test", "1"),
        );
        crate::router::service::Service::new(document, move |_| {
            let calls = Arc::clone(&calls);
            let release = Arc::clone(&release);
            async move {
                let call = calls.fetch_add(1, Ordering::SeqCst);
                if call == 0 {
                    release.notified().await;
                }
                crate::http::Response::new(crate::http::body::Body::from_bytes(
                    bytes::Bytes::from_static(b"ok"),
                ))
            }
        })
    };
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(service)
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .max_connections(NonZeroUsize::new(1).expect("one is non-zero"))
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());

    let first = tokio::task::spawn_blocking(move || request_http1(address));
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while calls.load(Ordering::SeqCst) != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the first request starts");
    let second = tokio::task::spawn_blocking(move || request_http1(address));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), async {
            while calls.load(Ordering::SeqCst) != 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .is_err(),
        "the second connection must remain in the listener backlog"
    );

    release.notify_one();
    first.await.expect("first client joins");
    second.await.expect("second client joins");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

#[cfg(feature = "http1")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn zero_shutdown_timeout_reports_an_incomplete_drain() {
    use std::sync::Arc;

    let started = Arc::new(tokio::sync::Notify::new());
    let service: crate::router::service::Service<()> = {
        let started = Arc::clone(&started);
        let document = kynos_openapi::Document::new(
            kynos_openapi::SpecVersion::V3_1,
            kynos_openapi::Info::new("Test", "1"),
        );
        crate::router::service::Service::new(document, move |_| {
            let started = Arc::clone(&started);
            async move {
                started.notify_one();
                std::future::pending().await
            }
        })
    };
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(service)
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .shutdown_timeout(std::time::Duration::ZERO)
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());
    let client = tokio::task::spawn_blocking(move || request_http1(address));

    tokio::time::timeout(std::time::Duration::from_secs(1), started.notified())
        .await
        .expect("the request reaches the handler");
    let _ = shutdown_sender.send(());
    let error = tokio::time::timeout(std::time::Duration::from_secs(1), server)
        .await
        .expect("forced shutdown is prompt")
        .expect("server task joins")
        .expect_err("an incomplete drain is reported");
    assert!(matches!(
        &error,
        crate::Error::Server(crate::server::error::ServerError::ShutdownTimeout { timeout })
            if timeout.is_zero()
    ));
    assert!(matches!(&error, crate::Error::Server(error) if error.is_requested_shutdown()));
    client.await.expect("client task joins");
}

#[cfg(feature = "http1")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_shutdown_trigger_forces_an_incomplete_drain() {
    let (service, started, _release) = blocking_service();
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let (force_sender, force_receiver) = tokio::sync::oneshot::channel();
    let shutdown = crate::server::shutdown::Shutdown {
        future: Box::pin(async move {
            let _ = shutdown_receiver.await;
            Ok(crate::server::shutdown::ShutdownRequest {
                force: Box::pin(async move {
                    let _ = force_receiver.await;
                }),
            })
        }),
    };
    let bound = crate::server::Server::new(service)
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .shutdown_timeout(std::time::Duration::from_secs(25))
        .graceful_shutdown(shutdown)
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());
    let client = tokio::task::spawn_blocking(move || request_http1(address));

    tokio::time::timeout(std::time::Duration::from_secs(1), started.notified())
        .await
        .expect("the request reaches the handler");
    let _ = shutdown_sender.send(());
    tokio::task::yield_now().await;
    assert!(!server.is_finished(), "the request starts draining");
    let _ = force_sender.send(());

    let error = tokio::time::timeout(std::time::Duration::from_secs(1), server)
        .await
        .expect("forced shutdown is prompt")
        .expect("server task joins")
        .expect_err("the repeated trigger is reported");
    assert!(matches!(
        &error,
        crate::Error::Server(crate::server::error::ServerError::ShutdownForced)
    ));
    assert!(matches!(&error, crate::Error::Server(error) if error.is_requested_shutdown()));
    client.await.expect("client task joins");
}

/// A zero drain deadline reports `ShutdownTimeout` even when nothing was in
/// flight, as `shutdown_timeout` and `is_requested_shutdown` both say.
#[tokio::test]
async fn zero_shutdown_timeout_reports_a_timeout_with_nothing_in_flight() {
    let error = crate::server::Server::new(test_service())
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .shutdown_timeout(std::time::Duration::ZERO)
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async {}))
        .serve()
        .await
        .expect_err("a zero drain deadline is reported");

    assert!(
        matches!(
            &error,
            crate::Error::Server(crate::server::error::ServerError::ShutdownTimeout { timeout })
                if timeout.is_zero()
        ),
        "{error:?}"
    );
}

/// A trigger that fails, as an operating-system signal that cannot be
/// registered does, ends serving with `ServerError::Signal` carrying that
/// failure, rather than serving on without a way to stop.
#[tokio::test]
async fn a_failing_shutdown_trigger_ends_serving_with_its_error() {
    use std::io;

    let shutdown = crate::server::shutdown::Shutdown {
        future: Box::pin(async { Err(io::Error::other("no signal for you")) }),
    };
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        crate::server::Server::new(test_service())
            .bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .graceful_shutdown(shutdown)
            .serve(),
    )
    .await
    .expect("a failed trigger ends serving")
    .expect_err("the failed trigger is reported");

    let crate::Error::Server(crate::server::error::ServerError::Signal(source)) = &error else {
        panic!("not a signal failure: {error:?}");
    };
    assert_eq!(source.to_string(), "no signal for you");
    assert!(matches!(&error, crate::Error::Server(error) if !error.is_requested_shutdown()));
}

/// `Shutdown::signals` over real signals: the first `SIGTERM` stops the
/// listener while a request drains, and a second forces the drain to end.
///
/// The signals go to this process alone, which nextest gives this test to
/// itself. A current-thread runtime makes the order certain: `serve` registers
/// its listeners on the poll that spawns the accept loops, so no request can
/// reach the handler, and no signal be sent, before they exist.
#[cfg(all(unix, feature = "http1"))]
#[tokio::test(flavor = "current_thread")]
async fn a_second_termination_signal_forces_the_drain() {
    use rustix::process::{Signal, getpid, kill_process};

    let (service, started, _release) = blocking_service();
    let bound = crate::server::Server::new(service)
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .shutdown_timeout(std::time::Duration::from_secs(25))
        .graceful_shutdown(crate::server::shutdown::Shutdown::signals())
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());
    let client = tokio::task::spawn_blocking(move || request_http1(address));

    tokio::time::timeout(std::time::Duration::from_secs(5), started.notified())
        .await
        .expect("the request reaches the handler");
    kill_process(getpid(), Signal::TERM).expect("the first signal is sent");

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while tokio::net::TcpStream::connect(address).await.is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the first signal stops the listener");
    assert!(!server.is_finished(), "the request is still draining");

    kill_process(getpid(), Signal::TERM).expect("the second signal is sent");
    let error = tokio::time::timeout(std::time::Duration::from_secs(5), server)
        .await
        .expect("the second signal ends the drain promptly")
        .expect("server task joins")
        .expect_err("the forced drain is reported");
    assert!(
        matches!(
            &error,
            crate::Error::Server(crate::server::error::ServerError::ShutdownForced)
        ),
        "{error:?}"
    );
    client.await.expect("client task joins");
}

/// Only the two incomplete-drain variants are a requested shutdown; every
/// other one is a server that stopped on its own or never started.
///
/// [`server_error_variant`] names every variant in an exhaustive match, so a
/// variant added to `ServerError` stops this compiling until it is named;
/// the list below is what catches one named and given no value.
#[test]
fn only_an_incomplete_drain_is_a_requested_shutdown() {
    use std::{io, net::Ipv4Addr};

    use crate::server::error::ServerError;

    let io_error = || io::Error::other("failure");
    let address = (Ipv4Addr::LOCALHOST, 0).into();
    let every = [
        ServerError::NoListeners,
        ServerError::InvalidConfiguration("setting"),
        ServerError::Resolve {
            address: crate::server::address::BindAddress::from(address),
            source: io_error(),
        },
        ServerError::Bind {
            address,
            source: io_error(),
        },
        ServerError::Listener(io_error()),
        ServerError::Accept {
            address,
            source: io_error(),
        },
        ServerError::AcceptLoop { panicked: true },
        ServerError::Signal(io_error()),
        ServerError::ShutdownTimeout {
            timeout: std::time::Duration::ZERO,
        },
        ServerError::ShutdownForced,
        #[cfg(feature = "tls")]
        ServerError::MutualTlsConflict,
        #[cfg(feature = "tls")]
        ServerError::Tls(crate::server::tls::error::TlsError::ZeroHandshakeTimeout),
    ];

    let witnessed = every.iter().map(server_error_variant).collect::<Vec<_>>();
    assert_eq!(
        witnessed,
        [
            "NoListeners",
            "InvalidConfiguration",
            "Resolve",
            "Bind",
            "Listener",
            "Accept",
            "AcceptLoop",
            "Signal",
            "ShutdownTimeout",
            "ShutdownForced",
            #[cfg(feature = "tls")]
            "MutualTlsConflict",
            #[cfg(feature = "tls")]
            "Tls",
        ],
        "a variant has no value"
    );
    for error in &every {
        assert_eq!(
            error.is_requested_shutdown(),
            matches!(
                server_error_variant(error),
                "ShutdownTimeout" | "ShutdownForced"
            ),
            "{error:?}"
        );
    }
}

/// The name of `error`'s variant, with no wildcard arm.
fn server_error_variant(error: &crate::server::error::ServerError) -> &'static str {
    use crate::server::error::ServerError;

    match error {
        ServerError::NoListeners => "NoListeners",
        ServerError::InvalidConfiguration(_) => "InvalidConfiguration",
        ServerError::Resolve { .. } => "Resolve",
        ServerError::Bind { .. } => "Bind",
        ServerError::Listener(_) => "Listener",
        ServerError::Accept { .. } => "Accept",
        ServerError::AcceptLoop { .. } => "AcceptLoop",
        ServerError::Signal(_) => "Signal",
        ServerError::ShutdownTimeout { .. } => "ShutdownTimeout",
        ServerError::ShutdownForced => "ShutdownForced",
        #[cfg(feature = "tls")]
        ServerError::MutualTlsConflict => "MutualTlsConflict",
        #[cfg(feature = "tls")]
        ServerError::Tls(_) => "Tls",
    }
}

/// Makes every later `accept` on `listener` fail the way that ends its loop:
/// on Linux, shutting a listening socket down for reading leaves it refusing
/// `accept` with `EINVAL`, an error the backoff gives up on at once.
/// `listener` is a duplicate of the one the server holds, so it reaches the
/// same socket.
#[cfg(all(target_os = "linux", feature = "http1"))]
fn break_listener(listener: &std::net::TcpListener) {
    socket2::SockRef::from(listener)
        .shutdown(std::net::Shutdown::Read)
        .expect("a listening socket shuts down");
}

/// A server whose listener `break_listener` can reach, serving `service`.
#[cfg(all(target_os = "linux", feature = "http1"))]
async fn breakable_server(
    service: crate::router::service::Service<()>,
    shutdown_timeout: std::time::Duration,
) -> (
    std::net::SocketAddr,
    std::net::TcpListener,
    tokio::task::JoinHandle<crate::Result<()>>,
) {
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("loopback listener binds");
    let handle = listener.try_clone().expect("listener duplicates");
    let bound = crate::server::Server::new(service)
        .listener(listener)
        .shutdown_timeout(shutdown_timeout)
        .prepare()
        .await
        .expect("standard listener converts to Tokio ownership");
    let address = bound.local_addrs()[0];
    (address, handle, tokio::spawn(bound.serve()))
}

#[cfg(all(target_os = "linux", feature = "http1"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_listener_that_stops_accepting_drains_what_it_accepted() {
    let (service, started, release) = blocking_service();
    let (address, listener, server) =
        breakable_server(service, std::time::Duration::from_secs(25)).await;
    let client = tokio::task::spawn_blocking(move || request_http1(address));

    tokio::time::timeout(std::time::Duration::from_secs(1), started.notified())
        .await
        .expect("the request reaches the handler");
    break_listener(&listener);
    // The backoff gives up on the first `EINVAL`; this outlasts it with
    // margin, so the listener has certainly failed before the request is let go.
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    assert!(
        !server.is_finished(),
        "the accepted request must keep draining"
    );

    release.notify_one();
    let response = client.await.expect("client task joins");
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.ends_with("ok"), "{response}");
    let error = tokio::time::timeout(std::time::Duration::from_secs(1), server)
        .await
        .expect("the server exits once its connections drain")
        .expect("server task joins")
        .expect_err("the failed listener is reported");
    assert!(
        matches!(
            error,
            crate::Error::Server(crate::server::error::ServerError::Accept { address: failed, .. })
                if failed == address
        ),
        "{error:?}"
    );
}

#[cfg(all(target_os = "linux", feature = "http1"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_listener_that_stops_accepting_drains_under_the_shutdown_timeout() {
    let (service, started, _release) = blocking_service();
    let (address, listener, server) =
        breakable_server(service, std::time::Duration::from_millis(200)).await;
    let client = tokio::task::spawn_blocking(move || request_http1(address));

    tokio::time::timeout(std::time::Duration::from_secs(1), started.notified())
        .await
        .expect("the request reaches the handler");
    break_listener(&listener);

    let error = tokio::time::timeout(std::time::Duration::from_secs(2), server)
        .await
        .expect("a request that never finishes is cut at the shutdown timeout")
        .expect("server task joins")
        .expect_err("the failed listener is reported");
    assert!(
        matches!(
            error,
            crate::Error::Server(crate::server::error::ServerError::Accept { address: failed, .. })
                if failed == address
        ),
        "{error:?}"
    );
    client.await.expect("client task joins");
}

#[cfg(feature = "http2")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http2_prior_knowledge_serves_over_a_real_socket() {
    use http_body_util::{BodyExt as _, Empty};
    use hyper_util::rt::{TokioExecutor, TokioIo};

    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(test_service())
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .expect("HTTP/2 handshake completes");
    let connection = tokio::spawn(connection);
    let request = hyper::Request::builder()
        .uri("http://localhost/")
        .body(Empty::<bytes::Bytes>::new())
        .expect("request builds");
    let response = sender
        .send_request(request)
        .await
        .expect("request succeeds");
    let body = response
        .into_body()
        .collect()
        .await
        .expect("response body reads")
        .to_bytes();
    assert_eq!(body, bytes::Bytes::from_static(b"ok"));

    drop(sender);
    connection
        .await
        .expect("client connection task joins")
        .expect("client connection closes cleanly");
    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// An HTTP/2 peer that stops answering is pinged, then disconnected.
///
/// The client is raw bytes rather than hyper's, because hyper's client
/// acknowledges every PING itself: the case under test is a peer that has
/// vanished, which a conforming client cannot play. It opens the connection
/// with no stream, acknowledges the server's SETTINGS so nothing else is owed,
/// and then reads until the server closes.
#[cfg(feature = "http2")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_http2_peer_that_never_acknowledges_a_ping_is_disconnected() {
    use std::time::Duration;

    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    const SETTINGS: u8 = 0x4;
    const PING: u8 = 0x6;
    const ACK: u8 = 0x1;

    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(test_service())
        .http2(Http2Config::default().keep_alive(Some(Http2KeepAlive::new(
            Duration::from_millis(100),
            Duration::from_millis(100),
        ))))
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());

    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    stream
        .write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n\0\0\0\x04\0\0\0\0\0")
        .await
        .expect("preface and empty SETTINGS send");

    let frames = tokio::time::timeout(Duration::from_secs(10), async {
        let mut seen = Vec::new();
        loop {
            let mut header = [0_u8; 9];
            if stream.read_exact(&mut header).await.is_err() {
                return seen;
            }
            let length =
                usize::from(header[0]) << 16 | usize::from(header[1]) << 8 | usize::from(header[2]);
            let mut payload = vec![0_u8; length];
            if stream.read_exact(&mut payload).await.is_err() {
                return seen;
            }
            let (kind, flags) = (header[3], header[4]);
            if kind == SETTINGS && flags & ACK == 0 {
                stream
                    .write_all(&[0, 0, 0, SETTINGS, ACK, 0, 0, 0, 0])
                    .await
                    .expect("SETTINGS acknowledgement sends");
            }
            seen.push((kind, flags));
        }
    })
    .await
    .expect("the server closes a connection whose PING goes unanswered");

    assert!(
        frames.contains(&(PING, 0)),
        "the server pinged the silent peer before closing: {frames:?}"
    );

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// The header-read or HTTP/2 idle timeout the silent-connection cases configure.
///
/// Short, so a case that passes is quick, and far below
/// [`SILENT_CONNECTION_BOUND`], so one that fails is not a slow pass.
#[cfg(any(feature = "http1", feature = "http2"))]
const HEAD_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(200);

/// How long a silent-connection case waits for the server to close it.
#[cfg(any(feature = "http1", feature = "http2"))]
const SILENT_CONNECTION_BOUND: std::time::Duration = std::time::Duration::from_secs(5);

/// The first-head deadline is the header-read timeout counted from accept, and
/// there is none when the timeout is disabled or would overflow the clock.
#[cfg(feature = "http1")]
#[test]
fn the_first_head_deadline_counts_the_header_read_timeout_from_accept() {
    use std::time::{Duration, Instant};

    let accepted = Instant::now();
    let deadline = |timeout| {
        Http1Config::default()
            .header_read_timeout(timeout)
            .first_head_deadline(accepted)
    };

    assert_eq!(deadline(Some(HEAD_TIMEOUT)), Some(accepted + HEAD_TIMEOUT));
    assert_eq!(deadline(None), None);
    assert_eq!(deadline(Some(Duration::MAX)), None);
}

/// HTTP/2's share of the first-head deadline is its idle timeout counted from
/// accept, with the same two cases of none.
#[cfg(feature = "http2")]
#[test]
fn the_http2_first_head_deadline_counts_the_idle_timeout_from_accept() {
    use std::time::{Duration, Instant};

    let accepted = Instant::now();
    let deadline = |timeout| {
        Http2Config::default()
            .idle_timeout(timeout)
            .first_head_deadline(accepted)
    };

    assert_eq!(deadline(Some(HEAD_TIMEOUT)), Some(accepted + HEAD_TIMEOUT));
    assert_eq!(deadline(None), None);
    assert_eq!(deadline(Some(Duration::MAX)), None);
}

/// With both protocols compiled, a connection's first-head deadline is the
/// earlier of the header-read and idle deadlines, either alone when the other
/// is disabled, and none when both are.
#[cfg(all(feature = "http1", feature = "http2"))]
#[test]
fn the_first_head_deadline_is_the_earlier_of_both_protocols() {
    use std::{num::NonZeroUsize, time::Instant};

    let accepted = Instant::now();
    let later = HEAD_TIMEOUT * 2;
    let deadline = |header_read, idle| {
        let config = super::TransportConfig {
            http1: Http1Config::default().header_read_timeout(header_read),
            http2: Http2Config::default().idle_timeout(idle),
            #[cfg(feature = "tls")]
            tls: None,
            shutdown_timeout: std::time::Duration::from_secs(25),
            max_connections: NonZeroUsize::MIN,
            request_body_idle_timeout: None,
        };
        crate::server::connection::first_head_deadline(&config, accepted)
    };

    let first = Some(accepted + HEAD_TIMEOUT);
    assert_eq!(deadline(Some(HEAD_TIMEOUT), Some(later)), first);
    assert_eq!(deadline(Some(later), Some(HEAD_TIMEOUT)), first);
    assert_eq!(deadline(None, Some(HEAD_TIMEOUT)), first);
    assert_eq!(deadline(Some(HEAD_TIMEOUT), None), first);
    assert_eq!(deadline(None, None), None);
}

/// The idle timeout the `Streams::idle` cases wait out, on a paused clock.
#[cfg(feature = "http2")]
const IDLE: std::time::Duration = std::time::Duration::from_secs(10);

/// Streams that have opened and closed one stream, and the instant the idle
/// wait over them started, which is when that stream closed.
#[cfg(feature = "http2")]
fn quiet_streams() -> (
    std::sync::Arc<crate::server::protocol::http2::Streams>,
    tokio::task::JoinHandle<tokio::time::Instant>,
    tokio::time::Instant,
) {
    let streams = std::sync::Arc::new(crate::server::protocol::http2::Streams::default());
    drop(streams.open());
    let started = tokio::time::Instant::now();
    let waiting = std::sync::Arc::clone(&streams);
    let idle = tokio::spawn(async move {
        waiting.idle(Some(IDLE)).await;
        tokio::time::Instant::now()
    });
    (streams, idle, started)
}

/// A stream that opens and finishes inside the idle wait restarts the idle
/// period from its close, so the connection idles a whole timeout after its
/// last stream and no longer.
#[cfg(feature = "http2")]
#[tokio::test(start_paused = true)]
async fn a_stream_finished_during_the_idle_wait_restarts_it_from_its_close() {
    let (streams, idle, started) = quiet_streams();

    tokio::time::sleep(IDLE * 2 / 5).await;
    drop(streams.open());
    let closed = tokio::time::Instant::now();

    let idled = idle.await.expect("the idle wait does not panic");
    assert_eq!(idled - started, closed - started + IDLE);
}

/// A stream that opens inside the idle wait and is still in flight when the
/// wait's timer ends holds the connection until a whole timeout after it
/// closes.
#[cfg(feature = "http2")]
#[tokio::test(start_paused = true)]
async fn a_stream_opened_during_the_idle_wait_holds_it_until_the_stream_closes() {
    let (streams, idle, started) = quiet_streams();

    tokio::time::sleep(IDLE * 2 / 5).await;
    let in_flight = streams.open();
    tokio::time::sleep(IDLE * 3).await;
    assert!(
        !idle.is_finished(),
        "a connection with a stream in flight is not idle"
    );
    drop(in_flight);
    let closed = tokio::time::Instant::now();

    let idled = idle.await.expect("the idle wait does not panic");
    assert_eq!(idled - started, closed - started + IDLE);
}

/// A counted response body reports its inner body's end and size, so hyper
/// still ends an empty HTTP/2 response on its HEADERS frame.
#[cfg(feature = "http2")]
#[test]
fn a_counted_body_reports_its_inner_end_and_size() {
    use hyper::body::Body as _;

    use crate::{http::body::Body, server::connection::Counted};

    let counted = |body| Counted {
        body,
        _in_flight: None,
    };

    let empty = counted(Body::empty());
    assert!(empty.is_end_stream(), "an empty body is over");
    assert_eq!(empty.size_hint().exact(), Some(0));

    let full = counted(Body::from("abc"));
    assert!(
        !full.is_end_stream(),
        "a body with a frame left is not over"
    );
    assert_eq!(full.size_hint().exact(), Some(3));
}

/// A plaintext server whose header-read timeout is [`HEAD_TIMEOUT`].
#[cfg(feature = "http1")]
async fn head_timed_server() -> (
    std::net::SocketAddr,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<crate::error::Result<()>>,
) {
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(test_service())
        .http1(Http1Config::default().header_read_timeout(Some(HEAD_TIMEOUT)))
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    (address, shutdown_sender, tokio::spawn(bound.serve()))
}

/// Reads `stream` until the server closes it, failing past
/// [`SILENT_CONNECTION_BOUND`].
///
/// The read's own result is discarded: a closed connection reports zero bytes
/// and a reset one an error, and both are the server letting go of it.
#[cfg(any(feature = "http1", feature = "http2"))]
async fn assert_server_closes(stream: &mut (impl tokio::io::AsyncRead + Unpin), case: &str) {
    use tokio::io::AsyncReadExt as _;

    let mut discarded = Vec::new();
    let closed =
        tokio::time::timeout(SILENT_CONNECTION_BOUND, stream.read_to_end(&mut discarded)).await;
    assert!(
        closed.is_ok(),
        "{case}: the server still holds the connection {SILENT_CONNECTION_BOUND:?} after \
         accepting it, past a {HEAD_TIMEOUT:?} timeout"
    );
}

/// A connection that never sends a byte is closed at the header-read timeout.
///
/// With both protocols compiled and no ALPN, hyper-util reads the first bytes
/// to choose a codec, and hyper's own header-read timer only starts once that
/// codec runs -- so the wait before it, which is the whole of this
/// connection's life, had no timer, and it held a `max_connections` permit for
/// as long as the client kept the socket open.
#[cfg(feature = "http1")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_connection_that_never_speaks_is_closed_at_the_header_read_timeout() {
    let (address, shutdown_sender, server) = head_timed_server().await;

    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    assert_server_closes(&mut stream, "a silent connection").await;

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// A connection that stops inside the HTTP/2 preface is closed the same way.
///
/// `PRI` is a prefix of the preface, so the protocol sniff can decide nothing
/// from it and waits for the rest, which never comes.
#[cfg(feature = "http1")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_connection_that_stops_inside_the_http2_preface_is_closed() {
    use tokio::io::AsyncWriteExt as _;

    let (address, shutdown_sender, server) = head_timed_server().await;

    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    stream.write_all(b"PRI").await.expect("the prefix writes");
    assert_server_closes(&mut stream, "a connection stopped inside the preface").await;

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// An HTTP/2 connection that finishes its preface and opens no stream is
/// closed at the header-read timeout.
///
/// hyper's client answers every PING itself, which is the peer the issue
/// describes: keep-alive cannot tell it from a live one, and HTTP/2 has no
/// header-read timer of its own, so the first request head is what it is held
/// to.
#[cfg(all(feature = "http1", feature = "http2"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_http2_connection_that_opens_no_stream_is_closed() {
    use hyper_util::rt::{TokioExecutor, TokioIo};

    let (address, shutdown_sender, server) = head_timed_server().await;

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let (sender, connection) = hyper::client::conn::http2::handshake::<
        _,
        _,
        http_body_util::Empty<bytes::Bytes>,
    >(TokioExecutor::new(), TokioIo::new(stream))
    .await
    .expect("HTTP/2 handshake completes");
    // The connection future ends once the server closes the socket, whatever
    // it reports doing so.
    let closed = tokio::time::timeout(SILENT_CONNECTION_BOUND, connection).await;
    assert!(
        closed.is_ok(),
        "an HTTP/2 connection with no stream is still held {SILENT_CONNECTION_BOUND:?} after \
         accepting it, past a {HEAD_TIMEOUT:?} header-read timeout"
    );
    drop(sender);

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// With the header-read timeout disabled, an HTTP/2 connection that finishes
/// its preface and opens no stream is closed at the HTTP/2 idle timeout: no
/// stream is in flight from accept, so the idle timeout bounds the first head
/// as well.
#[cfg(all(feature = "http1", feature = "http2"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_http2_connection_that_opens_no_stream_is_closed_at_the_idle_timeout() {
    use hyper_util::rt::{TokioExecutor, TokioIo};

    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(test_service())
        .http1(Http1Config::default().header_read_timeout(None))
        .http2(Http2Config::default().idle_timeout(Some(HEAD_TIMEOUT)))
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let (sender, connection) = hyper::client::conn::http2::handshake::<
        _,
        _,
        http_body_util::Empty<bytes::Bytes>,
    >(TokioExecutor::new(), TokioIo::new(stream))
    .await
    .expect("HTTP/2 handshake completes");
    let closed = tokio::time::timeout(SILENT_CONNECTION_BOUND, connection).await;
    assert!(
        closed.is_ok(),
        "an HTTP/2 connection with no stream is still held {SILENT_CONNECTION_BOUND:?} after \
         accepting it, past a {HEAD_TIMEOUT:?} idle timeout and no header-read timeout"
    );
    drop(sender);

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// The bound is on the first request head only: a connection that produced one
/// in time is not closed when the timeout passes.
///
/// HTTP/2 rather than HTTP/1, since an idle HTTP/1 connection is held to the
/// header-read timeout between requests by hyper itself; an HTTP/2 one is held
/// only to its idle timeout, 30 s by default, so a second request long after
/// the first still finds the connection open only if the bound stopped at the
/// first head.
#[cfg(all(feature = "http1", feature = "http2"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_connection_that_sent_a_request_outlives_the_header_read_timeout() {
    use http_body_util::Empty;
    use hyper_util::rt::{TokioExecutor, TokioIo};

    let (address, shutdown_sender, server) = head_timed_server().await;

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .expect("HTTP/2 handshake completes");
    let connection = tokio::spawn(connection);
    let request = || {
        hyper::Request::builder()
            .uri("http://localhost/")
            .body(Empty::<bytes::Bytes>::new())
            .expect("request builds")
    };

    sender
        .send_request(request())
        .await
        .expect("the first request succeeds");
    tokio::time::sleep(HEAD_TIMEOUT * 3).await;
    sender
        .send_request(request())
        .await
        .expect("the connection survives its header-read timeout once a request arrived");

    drop(sender);
    connection.abort();
    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// A plaintext server whose HTTP/2 idle timeout is [`HEAD_TIMEOUT`], serving
/// `service`.
#[cfg(feature = "http2")]
async fn idle_timed_server(
    service: crate::router::service::Service<()>,
    http2: Http2Config,
) -> (
    std::net::SocketAddr,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<crate::error::Result<()>>,
) {
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(service)
        .http2(http2.idle_timeout(Some(HEAD_TIMEOUT)))
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    (address, shutdown_sender, tokio::spawn(bound.serve()))
}

/// An HTTP/2 connection that answers every PING and opens no stream after its
/// first is sent a GOAWAY and closed at the idle timeout.
///
/// Raw bytes rather than hyper's client, so the frames the server sent can be
/// read: the GOAWAY is what tells a conforming client not to open another
/// stream on a connection about to close. The one request is a HEADERS frame
/// of three static-table indices -- `GET`, `http` and `/` -- ending its stream.
/// Keep-alive pings every 50 ms and the client acknowledges each, which is the
/// peer keep-alive cannot tell from a live one.
#[cfg(feature = "http2")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_idle_http2_connection_that_answers_pings_is_sent_a_goaway_and_closed() {
    use std::time::Duration;

    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    const HEADERS: u8 = 0x1;
    const SETTINGS: u8 = 0x4;
    const PING: u8 = 0x6;
    const GOAWAY: u8 = 0x7;
    const ACK: u8 = 0x1;

    let keep_alive = Http2KeepAlive::new(Duration::from_millis(50), Duration::from_secs(1));
    let (address, shutdown_sender, server) = idle_timed_server(
        test_service(),
        Http2Config::default().keep_alive(Some(keep_alive)),
    )
    .await;

    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    stream
        .write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n\0\0\0\x04\0\0\0\0\0")
        .await
        .expect("preface and empty SETTINGS send");
    stream
        .write_all(&[0, 0, 3, HEADERS, 0x5, 0, 0, 0, 1, 0x82, 0x86, 0x84])
        .await
        .expect("one request sends");

    let frames = tokio::time::timeout(SILENT_CONNECTION_BOUND, async {
        let mut seen = Vec::new();
        loop {
            let mut header = [0_u8; 9];
            if stream.read_exact(&mut header).await.is_err() {
                return seen;
            }
            let length =
                usize::from(header[0]) << 16 | usize::from(header[1]) << 8 | usize::from(header[2]);
            let mut payload = vec![0_u8; length];
            if stream.read_exact(&mut payload).await.is_err() {
                return seen;
            }
            let (kind, flags) = (header[3], header[4]);
            if kind == SETTINGS && flags & ACK == 0 {
                stream
                    .write_all(&[0, 0, 0, SETTINGS, ACK, 0, 0, 0, 0])
                    .await
                    .expect("SETTINGS acknowledgement sends");
            }
            if kind == PING && flags & ACK == 0 {
                let mut ack = vec![0, 0, 8, PING, ACK, 0, 0, 0, 0];
                ack.extend_from_slice(&payload);
                // The server may already have closed; the read below says so.
                let _ = stream.write_all(&ack).await;
            }
            seen.push(kind);
        }
    })
    .await
    .expect("the server closes a connection that holds no stream past its idle timeout");

    let answered = frames
        .iter()
        .position(|kind| *kind == HEADERS)
        .expect("the request was answered before the connection idled");
    let goaway = frames
        .iter()
        .rposition(|kind| *kind == GOAWAY)
        .expect("the server sent a GOAWAY before closing");
    assert!(answered < goaway, "{frames:?}");

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// A stream is in flight until its response body ends, not only while its
/// handler runs: a body that outlasts the idle timeout keeps the connection
/// open for the next request.
#[cfg(feature = "http2")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_response_body_that_outlasts_the_idle_timeout_holds_the_connection() {
    use http_body_util::{BodyExt as _, Empty};
    use hyper_util::rt::{TokioExecutor, TokioIo};

    let (service, release) = held_body_service();
    let (address, shutdown_sender, server) =
        idle_timed_server(service, Http2Config::default()).await;

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .expect("HTTP/2 handshake completes");
    let connection = tokio::spawn(connection);
    let request = || {
        hyper::Request::builder()
            .uri("http://localhost/")
            .body(Empty::<bytes::Bytes>::new())
            .expect("request builds")
    };

    let held = sender
        .send_request(request())
        .await
        .expect("the held response's head arrives");
    tokio::time::sleep(HEAD_TIMEOUT * 3).await;
    let _ = release.send(());
    let body = held
        .into_body()
        .collect()
        .await
        .expect("the held body finishes")
        .to_bytes();
    assert_eq!(body, bytes::Bytes::from_static(b"ok"));
    sender
        .send_request(request())
        .await
        .expect("the connection was not idle while the body was in flight");

    drop(sender);
    connection.abort();
    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// A build without HTTP/1 has no header-read timeout, so the HTTP/2 idle
/// timeout bounds a connection's wait for its first request head instead.
#[cfg(all(feature = "http2", not(feature = "http1")))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_http2_only_connection_that_never_speaks_is_closed_at_the_idle_timeout() {
    let (address, shutdown_sender, server) =
        idle_timed_server(test_service(), Http2Config::default()).await;

    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    assert_server_closes(&mut stream, "a silent HTTP/2-only connection").await;

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// A stream the server is serving when shutdown starts is answered before the
/// connection closes.
///
/// The client must have taken the response in hand before the GOAWAY reaches
/// it, which hyper's does late: its dispatch task hands the request to `h2`,
/// and only on its next run spawns the future that awaits the response. That
/// run first checks whether the connection can open another stream, and gives
/// up on any GOAWAY already received, failing a request the server is still
/// serving with `GoAway(NO_ERROR, Remote)`. The task reads its next request
/// only after that run, so an answered probe on the same connection shows the
/// response is awaited before shutdown is triggered.
#[cfg(feature = "http2")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_drains_an_active_http2_stream() {
    use http_body_util::{BodyExt as _, Empty};
    use hyper_util::rt::{TokioExecutor, TokioIo};

    const PROBE: &str = "/probe";

    let started = std::sync::Arc::new(tokio::sync::Notify::new());
    let release = std::sync::Arc::new(tokio::sync::Notify::new());
    // `blocking_service`, except that the probe is answered at once.
    let service: crate::router::service::Service<()> = {
        let started = std::sync::Arc::clone(&started);
        let release = std::sync::Arc::clone(&release);
        let document = kynos_openapi::Document::new(
            kynos_openapi::SpecVersion::V3_1,
            kynos_openapi::Info::new("Test", "1"),
        );
        crate::router::service::Service::new(document, move |request: crate::http::Request| {
            let held = (request.uri().path() != PROBE).then(|| {
                (
                    std::sync::Arc::clone(&started),
                    std::sync::Arc::clone(&release),
                )
            });
            async move {
                if let Some((started, release)) = held {
                    started.notify_one();
                    release.notified().await;
                }
                crate::http::Response::new(crate::http::body::Body::from_bytes(
                    bytes::Bytes::from_static(b"ok"),
                ))
            }
        })
    };
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(service)
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .expect("HTTP/2 handshake completes");
    let connection = tokio::spawn(connection);
    let request = |path: &str| {
        hyper::Request::builder()
            .uri(format!("http://localhost{path}"))
            .body(Empty::<bytes::Bytes>::new())
            .expect("request builds")
    };
    let mut probe = sender.clone();
    let drained = tokio::spawn(async move {
        sender
            .send_request(request("/"))
            .await
            .expect("request succeeds")
            .into_body()
            .collect()
            .await
            .expect("response body reads")
            .to_bytes()
    });

    tokio::time::timeout(std::time::Duration::from_secs(1), started.notified())
        .await
        .expect("the stream reaches the handler");
    probe
        .send_request(request(PROBE))
        .await
        .expect("the probe is answered before shutdown");
    drop(probe);
    let _ = shutdown_sender.send(());
    tokio::task::yield_now().await;
    assert!(
        !server.is_finished(),
        "the active stream must keep draining"
    );

    release.notify_one();
    assert_eq!(
        drained.await.expect("request task joins"),
        bytes::Bytes::from_static(b"ok")
    );
    connection
        .await
        .expect("client connection task joins")
        .expect("client connection closes cleanly");
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

#[cfg(feature = "tls")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_cancels_an_incomplete_tls_handshake() {
    use std::{num::NonZeroUsize, sync::Arc};

    let identity = server_identity();

    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("loopback listener binds");
    let address = listener.local_addr().expect("listener has an address");
    let client = tokio::spawn(tokio::net::TcpStream::connect(address));
    let (stream, peer_addr) = listener.accept().await.expect("server accepts");
    let _client = client
        .await
        .expect("client task joins")
        .expect("client connects");
    let config = super::TransportConfig {
        #[cfg(feature = "http1")]
        http1: super::Http1Config::default(),
        #[cfg(feature = "http2")]
        http2: super::Http2Config::default(),
        tls: Some(
            crate::server::tls::TlsConfig::from_pem(
                identity.certificate.as_bytes(),
                identity.key.as_bytes(),
            )
            .expect("TLS identity parses")
            .build()
            .expect("TLS config builds"),
        ),
        shutdown_timeout: std::time::Duration::from_secs(25),
        max_connections: NonZeroUsize::new(1).expect("one is non-zero"),
        request_body_idle_timeout: None,
    };
    let (stop_sender, stop_receiver) = tokio::sync::watch::channel(super::Lifecycle::Running);
    let mut connection = tokio::spawn(crate::server::connection::serve_connection(
        stream,
        peer_addr,
        address,
        Arc::new(test_service()),
        config,
        stop_receiver,
    ));

    tokio::task::yield_now().await;
    stop_sender.send_replace(super::Lifecycle::Draining);
    if tokio::time::timeout(std::time::Duration::from_secs(1), &mut connection)
        .await
        .is_err()
    {
        connection.abort();
        let _ = connection.await;
        panic!("the incomplete TLS handshake blocked shutdown");
    }
}

/// A completed TLS handshake that then says nothing does not hold the drain.
///
/// The case above covers a handshake that never finished. This is the one that
/// finished and fell silent: a pooled client's speculative pre-connect, or a
/// scanner that opens a socket and stops. It is a connection with nothing in
/// flight, so a drain must not wait for it.
///
/// It is the pin's failure mode, which is why it sits under the `http2` gate
/// and not under `tls` alone. hyper's HTTP/2 server cannot finish a graceful
/// shutdown before the client preface arrives -- `graceful_shutdown` in
/// `State::Handshaking` only sets `close_pending` (`hyper` 1.11.0
/// `src/proto/h2/server.rs`) -- so a connection pinned to `h2` the moment its
/// handshake ended stays `Pending`, the accept loop's drain never finishes,
/// `serve` waits out its whole shutdown timeout and returns
/// `ShutdownTimeout`. Deriving the protocol from the first bytes had no such
/// window: the driver owned the wait and cancelled its own read.
///
/// The client stream is held open across the assertion on purpose. Dropping it
/// would close the socket, complete the connection through EOF, and pass
/// whatever the server does with a client that stays.
#[cfg(all(feature = "tls", feature = "http2"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_drains_a_tls_connection_that_never_speaks() {
    use tokio_rustls::rustls::pki_types::ServerName;

    let (address, authority, shutdown_sender, server) = tls_server(test_service()).await;

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let client = alpn_connector(authority.as_bytes(), &[b"h2"])
        .connect(
            ServerName::try_from("localhost").expect("valid DNS name"),
            stream,
        )
        .await
        .expect("the h2 handshake succeeds");

    let _ = shutdown_sender.send(());
    // Well inside `DEFAULT_SHUTDOWN_TIMEOUT`, so waiting the timeout out reads
    // as this failing rather than as a slow drain.
    tokio::time::timeout(std::time::Duration::from_secs(5), server)
        .await
        .expect("the drain completes rather than waiting out the shutdown timeout")
        .expect("server task joins")
        .expect("server exits cleanly");

    drop(client);
}

#[cfg(feature = "tls")]
#[test]
fn mutual_tls_is_merged_into_every_security_alternative() {
    use kynos_openapi::{
        Document, Info, Method, Operation, PathItem, PathTemplate, SecurityRequirement, SpecVersion,
    };

    let mut document = Document::new(SpecVersion::V3_1, Info::new("Test", "1"));
    let mut operation = Operation::new("get_test");
    operation.security = Some(vec![SecurityRequirement::scheme("Bearer")]);
    let mut item = PathItem::new();
    item.set_operation(Method::Get, operation);
    document.paths.insert(
        &PathTemplate::parse("/test").expect("valid test path"),
        item,
    );

    crate::server::tls::document::apply_mutual_tls(&mut document)
        .expect("first contribution works");
    crate::server::tls::document::apply_mutual_tls(&mut document)
        .expect("contribution is idempotent");

    assert_eq!(document.security.len(), 1);
    assert!(
        document.security[0]
            .0
            .contains_key(crate::server::tls::document::MUTUAL_TLS_NAME)
    );
    let path = document
        .paths
        .get(&PathTemplate::parse("/test").expect("valid test path"))
        .expect("path exists");
    let requirements = path
        .get
        .as_ref()
        .and_then(|operation| operation.security.as_ref())
        .expect("operation overrides security");
    assert_eq!(requirements.len(), 1);
    assert!(requirements[0].0.contains_key("Bearer"));
    assert!(
        requirements[0]
            .0
            .contains_key(crate::server::tls::document::MUTUAL_TLS_NAME)
    );
}

#[cfg(feature = "tls")]
#[test]
fn mutual_tls_rejects_an_existing_incompatible_component() {
    use kynos_openapi::{ComponentName, Document, Info, SecurityScheme, SpecVersion};

    let mut document = Document::new(SpecVersion::V3_1, Info::new("Test", "1"));
    document.components.insert_security_scheme(
        &ComponentName::new(crate::server::tls::document::MUTUAL_TLS_NAME)
            .expect("built-in name is valid"),
        SecurityScheme::basic(),
    );

    assert!(matches!(
        crate::server::tls::document::apply_mutual_tls(&mut document),
        Err(crate::server::error::ServerError::MutualTlsConflict)
    ));
    assert!(document.security.is_empty());
}

/// Mutual TLS adds itself to the document `prepare` hands the bound server,
/// which it cannot do to a service a `serve_io` connection still shares, so
/// `prepare` refuses before binding rather than serve two documents under one
/// description.
#[cfg(feature = "tls")]
#[tokio::test]
async fn mutual_tls_refuses_a_service_a_connection_still_shares() {
    let issued = authority();
    let client_authentication =
        crate::server::tls::ClientCertificateConfig::from_pem_roots(issued.certificate.as_bytes())
            .expect("CA parses");
    let tls = crate::server::tls::TlsConfig::from_pem(
        issued.server.certificate.as_bytes(),
        issued.server.key.as_bytes(),
    )
    .expect("server identity parses")
    .require_client_certificate(client_authentication);

    let server = crate::server::Server::new(test_service());
    let (_client, io) = tokio::io::duplex(64);
    let connection = server
        .serve_io(io, "192.0.2.1:50000".parse().expect("a socket address"))
        .expect("the default configuration is accepted");
    let error = server
        .tls(tls)
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .prepare()
        .await
        .expect_err("a shared service cannot gain mutual TLS");

    assert!(matches!(
        error,
        crate::Error::Server(crate::server::error::ServerError::InvalidConfiguration(
            "mutual TLS adds itself to the document, so no serve_io connection may still share the service"
        ))
    ));
    drop(connection);
}

/// A mounted reference serves the description the bound server reports,
/// mutual TLS included.
///
/// `prepare` adds the scheme after `Router::build` rendered the reference, so
/// this is the one edit a reference can only carry if the served bytes follow
/// the document. Here rather than in `tests/docs.rs` because the bound server
/// hands its service to nothing public short of a mutual-TLS handshake.
#[cfg(all(feature = "tls", feature = "docs"))]
#[tokio::test]
async fn a_mounted_reference_serves_the_mutual_tls_the_bound_server_reports() {
    use http_body_util::BodyExt as _;

    let issued = authority();
    let client_authentication =
        crate::server::tls::ClientCertificateConfig::from_pem_roots(issued.certificate.as_bytes())
            .expect("CA parses");
    let tls = crate::server::tls::TlsConfig::from_pem(
        issued.server.certificate.as_bytes(),
        issued.server.key.as_bytes(),
    )
    .expect("server identity parses")
    .require_client_certificate(client_authentication);
    let service = crate::Router::<()>::new()
        .docs(crate::router::docs::Docs::scalar())
        .build(())
        .expect("a describable router");

    let bound = crate::server::Server::new(service)
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .tls(tls)
        .prepare()
        .await
        .expect("TLS listener prepares");
    let expected = bound.openapi().to_json().expect("a serializable document");
    assert!(
        expected.contains(crate::server::tls::document::MUTUAL_TLS_NAME),
        "the premise of this case is that the bound document declares mutual TLS"
    );

    let request = hyper::Request::get("/openapi.json")
        .body(crate::http::body::Body::empty())
        .expect("request builds");
    let reply = bound.service.call(request).await;
    let served = reply
        .into_body()
        .collect()
        .await
        .expect("a buffered description")
        .to_bytes();

    assert_eq!(String::from_utf8_lossy(&served), expected);
}

#[cfg(all(feature = "tls", feature = "http1"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// Long because it is one scenario end to end: a CA, a server identity, a client
// identity, a real socket and a verified round trip. Splitting it would put the
// setup out of sight of the assertion that depends on it.
#[expect(clippy::too_many_lines)]
async fn mutual_tls_serves_a_verified_client_over_a_real_socket() {
    use http_body_util::{BodyExt as _, Empty};
    use hyper_util::rt::TokioIo;
    use tokio_rustls::rustls::pki_types::{
        CertificateDer, PrivateKeyDer, ServerName, pem::PemObject as _,
    };

    let issued = authority();
    let ca = issued.certificate.as_bytes();

    let client_authentication =
        crate::server::tls::ClientCertificateConfig::from_pem_roots(ca).expect("CA parses");
    let tls = crate::server::tls::TlsConfig::from_pem(
        issued.server.certificate.as_bytes(),
        issued.server.key.as_bytes(),
    )
    .expect("server identity parses")
    .require_client_certificate(client_authentication);
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(test_service())
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .tls(tls)
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("TLS listener prepares");
    assert!(
        bound.openapi().security[0]
            .0
            .contains_key(crate::server::tls::document::MUTUAL_TLS_NAME)
    );
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());

    let anonymous_connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(
        client_config_builder()
            .with_root_certificates(trust_anchors(ca))
            .with_no_client_auth(),
    ));
    let anonymous_stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts anonymous socket");
    if let Ok(stream) = anonymous_connector
        .connect(
            ServerName::try_from("localhost").expect("valid DNS name"),
            anonymous_stream,
        )
        .await
    {
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .expect("client-side handshake can precede the server alert");
        let connection = tokio::spawn(connection);
        let request = hyper::Request::builder()
            .uri("/")
            .header(hyper::header::HOST, "localhost")
            .body(Empty::<bytes::Bytes>::new())
            .expect("request builds");
        assert!(
            sender.send_request(request).await.is_err(),
            "a client without a certificate must not exchange HTTP"
        );
        connection.abort();
    }

    let client_certificates = CertificateDer::pem_slice_iter(issued.client.certificate.as_bytes())
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("client chain parses");
    let client_key =
        PrivateKeyDer::from_pem_slice(issued.client.key.as_bytes()).expect("client key parses");
    let mut client_config = client_config_builder()
        .with_root_certificates(trust_anchors(ca))
        .with_client_auth_cert(client_certificates, client_key)
        .expect("client identity is valid");
    client_config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(client_config));
    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let stream = connector
        .connect(
            ServerName::try_from("localhost").expect("valid DNS name"),
            stream,
        )
        .await
        .expect("mutual TLS handshake succeeds");
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("HTTP/1 handshake completes");
    let connection = tokio::spawn(connection);
    let request = hyper::Request::builder()
        .uri("/")
        .header(hyper::header::HOST, "localhost")
        .body(Empty::<bytes::Bytes>::new())
        .expect("request builds");
    let body = sender
        .send_request(request)
        .await
        .expect("request succeeds")
        .into_body()
        .collect()
        .await
        .expect("response body reads")
        .to_bytes();
    assert_eq!(body, bytes::Bytes::from_static(b"ok"));

    drop(sender);
    connection.abort();
    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// A connection that settled on `http/1.1` is served over HTTP/1.
///
/// `serve_http` pins hyper's `auto` driver to the identifier the handshake
/// agreed rather than letting it read a protocol back off the first bytes of
/// the stream, and pinning the wrong half of a two-protocol offer would refuse
/// every client that chose the other -- so each half is served here, under its
/// own protocol's gate rather than under both, since a build carrying one
/// protocol pins that one and is where a mistake in its arm would ship alone.
///
/// The handler reports the ALPN identifier the connection carries alongside the
/// version the request arrived with, so a connection served as the *other*
/// protocol fails the comparison instead of passing it as "served at all".
///
/// Neither half distinguishes a pinned driver from a sniffing one: both answer
/// a client that speaks what it negotiated, and in a build carrying one
/// protocol the sniff can only reach the same answer the pin does.
/// `a_client_contradicting_its_negotiated_protocol_is_refused` is what the pin
/// can fail, and it needs both protocols compiled to say so.
#[cfg(all(feature = "tls", feature = "http1"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_serves_a_connection_that_settled_on_http1() {
    use http_body_util::{BodyExt as _, Empty};
    use hyper_util::rt::TokioIo;
    use tokio_rustls::rustls::pki_types::ServerName;

    let (address, authority, shutdown_sender, server) =
        tls_server(negotiated_protocol_service()).await;

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let stream = alpn_connector(authority.as_bytes(), &[b"http/1.1"])
        .connect(
            ServerName::try_from("localhost").expect("valid DNS name"),
            stream,
        )
        .await
        .expect("the HTTP/1.1 handshake succeeds");
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("HTTP/1 handshake completes");
    let connection = tokio::spawn(connection);
    let request = hyper::Request::builder()
        .uri("/")
        .header(hyper::header::HOST, "localhost")
        .body(Empty::<bytes::Bytes>::new())
        .expect("request builds");
    let response = sender
        .send_request(request)
        .await
        .expect("the request succeeds");
    assert_eq!(response.version(), hyper::Version::HTTP_11);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("response body reads")
        .to_bytes();
    assert_eq!(body, bytes::Bytes::from_static(b"http/1.1 HTTP/1.1"));
    drop(sender);
    connection.abort();

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// A connection pinned to HTTP/1.1 by ALPN still upgrades.
///
/// The pin is hyper's own HTTP/1 driver rather than hyper-util's, because
/// hyper-util's upgradeable driver ignores `http1_only` and sniffs; this is the
/// case that fails if the pinned arm serves without upgrades. The plaintext
/// half, which the sniffing driver serves, is `tests/unchecked.rs`'s.
#[cfg(all(feature = "tls", feature = "http1"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tls_connection_settled_on_http1_upgrades() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio_rustls::rustls::pki_types::ServerName;

    let document = kynos_openapi::Document::new(
        kynos_openapi::SpecVersion::V3_1,
        kynos_openapi::Info::new("Test", "1"),
    );
    let service = crate::router::service::Service::new(
        document,
        |mut request: crate::http::Request| async move {
            let upgrade = hyper::upgrade::on(&mut request);
            tokio::spawn(async move {
                let Ok(upgraded) = upgrade.await else {
                    return;
                };
                let mut io = hyper_util::rt::TokioIo::new(upgraded);
                let mut buffer = [0_u8; 64];
                while let Ok(read @ 1..) = io.read(&mut buffer).await {
                    if io.write_all(&buffer[..read]).await.is_err() {
                        return;
                    }
                }
            });
            let mut response = crate::http::Response::new(crate::http::body::Body::empty());
            *response.status_mut() = http::StatusCode::SWITCHING_PROTOCOLS;
            response.headers_mut().insert(
                http::header::CONNECTION,
                http::HeaderValue::from_static("upgrade"),
            );
            response.headers_mut().insert(
                http::header::UPGRADE,
                http::HeaderValue::from_static("echo"),
            );
            response
        },
    );
    let (address, authority, shutdown_sender, server) = tls_server(service).await;

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let mut stream = alpn_connector(authority.as_bytes(), &[b"http/1.1"])
        .connect(
            ServerName::try_from("localhost").expect("valid DNS name"),
            stream,
        )
        .await
        .expect("the HTTP/1.1 handshake succeeds");
    stream
        .write_all(
            b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: upgrade\r\nUpgrade: echo\r\n\r\n",
        )
        .await
        .expect("the request head writes");

    // Byte by byte, so nothing past the head is consumed.
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let byte = tokio::time::timeout(std::time::Duration::from_secs(5), stream.read_u8())
            .await
            .expect("the response head arrives")
            .expect("the response head reads");
        head.push(byte);
    }
    assert!(
        head.starts_with(b"HTTP/1.1 101"),
        "not a 101: {}",
        String::from_utf8_lossy(&head)
    );

    stream
        .write_all(b"ping")
        .await
        .expect("the upgraded stream writes");
    let mut echoed = [0_u8; 4];
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_exact(&mut echoed),
    )
    .await
    .expect("the echo arrives")
    .expect("the upgraded stream echoes rather than closes");
    assert_eq!(&echoed, b"ping");

    drop(stream);
    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// The other half of the offer: a connection that settled on `h2`.
///
/// Gated on `http2` alone for the reason the HTTP/1 case above gives, and it is
/// the half that matters most there: `h2` is the only identifier an
/// `http2`-only build offers, so nothing else in that build reaches the pin at
/// all.
#[cfg(all(feature = "tls", feature = "http2"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_serves_a_connection_that_settled_on_h2() {
    use http_body_util::{BodyExt as _, Empty};
    use hyper_util::rt::{TokioExecutor, TokioIo};
    use tokio_rustls::rustls::pki_types::ServerName;

    let (address, authority, shutdown_sender, server) =
        tls_server(negotiated_protocol_service()).await;

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let stream = alpn_connector(authority.as_bytes(), &[b"h2"])
        .connect(
            ServerName::try_from("localhost").expect("valid DNS name"),
            stream,
        )
        .await
        .expect("the h2 handshake succeeds");
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .expect("HTTP/2 handshake completes");
    let connection = tokio::spawn(connection);
    let request = hyper::Request::builder()
        .uri("https://localhost/")
        .body(Empty::<bytes::Bytes>::new())
        .expect("request builds");
    let response = sender
        .send_request(request)
        .await
        .expect("the request succeeds");
    assert_eq!(response.version(), hyper::Version::HTTP_2);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("response body reads")
        .to_bytes();
    assert_eq!(body, bytes::Bytes::from_static(b"h2 HTTP/2.0"));
    drop(sender);
    connection.abort();

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// A client that contradicts the protocol it negotiated is refused.
///
/// The whole of what pinning changes. While the driver read the protocol off
/// the first bytes of the stream, a connection that agreed on `h2` during the
/// handshake and then wrote an HTTP/1 request head was answered in HTTP/1 --
/// the wire overruling the handshake, and the connection carrying an ALPN
/// identifier its own traffic contradicts. rustls settled `h2`, so `h2` is what
/// the driver is given, and a request head that is not an HTTP/2 preface is not
/// a request.
///
/// The refusal is asserted as "no HTTP/1 response", not as a particular
/// failure: the driver may answer the malformed preface with a `GOAWAY`, close
/// the connection, or reset it, and all three are the same refusal.
#[cfg(all(feature = "tls", feature = "http1", feature = "http2"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_contradicting_its_negotiated_protocol_is_refused() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio_rustls::rustls::pki_types::ServerName;

    let (address, authority, shutdown_sender, server) =
        tls_server(negotiated_protocol_service()).await;

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let mut stream = alpn_connector(authority.as_bytes(), &[b"h2"])
        .connect(
            ServerName::try_from("localhost").expect("valid DNS name"),
            stream,
        )
        .await
        .expect("the h2 handshake succeeds");
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("the request head writes");
    stream.flush().await.expect("the request head flushes");

    // The read's own result is discarded: a reset connection reports an error
    // and a closed one reports zero bytes, and both are the same refusal. What
    // the case asserts is what arrived.
    let mut answer = Vec::new();
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_to_end(&mut answer),
    )
    .await
    .expect("the server answers or closes rather than holding the connection open");
    assert!(
        !answer.starts_with(b"HTTP/1.1"),
        "a connection that negotiated `h2` was served HTTP/1: {}",
        String::from_utf8_lossy(&answer)
    );
    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// One request over an established `stream`, as HTTP/2 when `http2` and as
/// HTTP/1.1 otherwise, and the body the server answered with.
#[cfg(all(feature = "tls", any(feature = "http1", feature = "http2")))]
async fn served_over<S>(stream: S, http2: bool) -> bytes::Bytes
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use http_body_util::{BodyExt as _, Empty};
    use hyper_util::rt::{TokioExecutor, TokioIo};

    let request = hyper::Request::builder()
        .uri("https://localhost/")
        .header(hyper::header::HOST, "localhost")
        .body(Empty::<bytes::Bytes>::new())
        .expect("request builds");
    let response = if http2 {
        let (mut sender, connection) =
            hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
                .await
                .expect("HTTP/2 handshake completes");
        tokio::spawn(connection);
        sender.send_request(request).await
    } else {
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .expect("HTTP/1 handshake completes");
        tokio::spawn(connection);
        sender.send_request(request).await
    };
    response
        .expect("the request succeeds")
        .into_body()
        .collect()
        .await
        .expect("response body reads")
        .to_bytes()
}

/// Offered both identifiers, in the order that puts HTTP/1.1 first, the
/// server settles on `h2`: its own preference decides, and the connection is
/// then served as HTTP/2.
#[cfg(all(feature = "tls", feature = "http1", feature = "http2"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_offering_both_protocols_is_served_h2() {
    use tokio_rustls::rustls::pki_types::ServerName;

    let (address, authority, shutdown_sender, server) =
        tls_server(negotiated_protocol_service()).await;

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let stream = alpn_connector(authority.as_bytes(), &[b"http/1.1", b"h2"])
        .connect(
            ServerName::try_from("localhost").expect("valid DNS name"),
            stream,
        )
        .await
        .expect("the handshake succeeds");
    assert_eq!(stream.get_ref().1.alpn_protocol(), Some(&b"h2"[..]));
    assert_eq!(
        served_over(stream, true).await,
        bytes::Bytes::from_static(b"h2 HTTP/2.0")
    );

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// A client offering no ALPN identifier settles nothing, so the server reads
/// the protocol off the first bytes, as on a plaintext listener: an HTTP/1.1
/// request is served as one, and so is an HTTP/2 preface sent with no
/// negotiation at all.
#[cfg(all(feature = "tls", any(feature = "http1", feature = "http2")))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_offering_no_protocol_is_served_what_it_speaks() {
    use tokio_rustls::rustls::pki_types::ServerName;

    let (address, authority, shutdown_sender, server) =
        tls_server(negotiated_protocol_service()).await;

    for (http2, expected) in [
        #[cfg(feature = "http1")]
        (false, "none HTTP/1.1"),
        #[cfg(feature = "http2")]
        (true, "none HTTP/2.0"),
    ] {
        let stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("server accepts");
        let stream = alpn_connector(authority.as_bytes(), &[])
            .connect(
                ServerName::try_from("localhost").expect("valid DNS name"),
                stream,
            )
            .await
            .expect("the handshake succeeds");
        assert_eq!(stream.get_ref().1.alpn_protocol(), None);
        assert_eq!(
            served_over(stream, http2).await,
            bytes::Bytes::from(expected),
            "HTTP/2: {http2}"
        );
    }

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// The certificate a handshake presents is the one registered for the name
/// the client sent: each name its own leaf, a certificate registered under
/// two names presented for either, a name compared without regard to case,
/// and the default for a name nothing registered.
#[cfg(feature = "tls")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_handshake_presents_the_certificate_registered_for_its_server_name() {
    use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName, pem::PemObject as _};

    let (issued, servers) =
        authority_issuing(&[&["a.example.test"], &["b.example.test", "c.example.test"]]);
    let tls = crate::server::tls::TlsConfig::from_pem(
        issued.server.certificate.as_bytes(),
        issued.server.key.as_bytes(),
    )
    .expect("server identity parses")
    .with_server_certificate(
        ["a.example.test"],
        servers[0].certificate.as_bytes(),
        servers[0].key.as_bytes(),
    )
    .expect("the first SNI identity registers")
    .with_server_certificate(
        ["B.Example.Test", "c.example.test"],
        servers[1].certificate.as_bytes(),
        servers[1].key.as_bytes(),
    )
    .expect("the second SNI identity registers");
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(test_service())
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .tls(tls)
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("TLS listener prepares");
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());

    let connector = alpn_connector(issued.certificate.as_bytes(), &[]);
    for (name, expected) in [
        ("a.example.test", &servers[0]),
        ("b.example.test", &servers[1]),
        ("C.EXAMPLE.TEST", &servers[1]),
        ("localhost", &issued.server),
    ] {
        let stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("server accepts");
        let stream = connector
            .connect(
                ServerName::try_from(name)
                    .expect("valid DNS name")
                    .to_owned(),
                stream,
            )
            .await
            .unwrap_or_else(|error| panic!("{name}: the handshake succeeds: {error}"));
        let presented = stream
            .get_ref()
            .1
            .peer_certificates()
            .and_then(<[CertificateDer<'_>]>::first)
            .expect("the server presents a certificate")
            .clone();
        assert_eq!(
            presented,
            CertificateDer::from_pem_slice(expected.certificate.as_bytes())
                .expect("the expected leaf parses"),
            "{name}"
        );
    }

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// A TLS connection that negotiated `protocol` and then says nothing is closed
/// at the header-read timeout, counted from accept.
///
/// The pin waits for the client's first byte before it builds a codec, and the
/// TLS handshake timeout ends before that wait starts, so the wait had no timer.
#[cfg(all(feature = "tls", feature = "http1"))]
async fn assert_silent_tls_connection_is_closed(protocol: &[u8]) {
    use tokio_rustls::rustls::pki_types::ServerName;

    let (address, authority, shutdown_sender, server) = tls_server_with(test_service(), |server| {
        server.http1(Http1Config::default().header_read_timeout(Some(HEAD_TIMEOUT)))
    })
    .await;

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let mut stream = alpn_connector(authority.as_bytes(), &[protocol])
        .connect(
            ServerName::try_from("localhost").expect("valid DNS name"),
            stream,
        )
        .await
        .expect("the handshake succeeds");
    assert_server_closes(&mut stream, "a silent pinned TLS connection").await;

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

#[cfg(all(feature = "tls", feature = "http1"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_silent_tls_connection_pinned_to_http1_is_closed() {
    assert_silent_tls_connection_is_closed(b"http/1.1").await;
}

#[cfg(all(feature = "tls", feature = "http1", feature = "http2"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_silent_tls_connection_pinned_to_h2_is_closed() {
    assert_silent_tls_connection_is_closed(b"h2").await;
}

/// A TLS listener serving `service`, and the authority a client must trust.
///
/// Both ALPN cases need the same three parts -- an authority, the server
/// identity it issued, and a bound listener -- and neither asserts anything
/// about any of them, so the setup is written once here.
#[cfg(feature = "tls")]
async fn tls_server(
    service: crate::router::service::Service<()>,
) -> (
    std::net::SocketAddr,
    String,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<crate::error::Result<()>>,
) {
    tls_server_with(service, |server| server).await
}

/// [`tls_server`], with `configure` applied to the server before it prepares.
#[cfg(feature = "tls")]
async fn tls_server_with(
    service: crate::router::service::Service<()>,
    configure: impl FnOnce(crate::server::Server<()>) -> crate::server::Server<()>,
) -> (
    std::net::SocketAddr,
    String,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<crate::error::Result<()>>,
) {
    let issued = authority();
    let tls = crate::server::tls::TlsConfig::from_pem(
        issued.server.certificate.as_bytes(),
        issued.server.key.as_bytes(),
    )
    .expect("server identity parses");
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = configure(crate::server::Server::new(service))
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .tls(tls)
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("TLS listener prepares");
    let address = bound.local_addrs()[0];

    (
        address,
        issued.certificate,
        shutdown_sender,
        tokio::spawn(bound.serve()),
    )
}

/// A client trusting `authority` and offering exactly `protocols` through ALPN.
///
/// The offer is the case's whole input: which identifier the handshake settles
/// on is what the server pins its driver to.
#[cfg(feature = "tls")]
fn alpn_connector(authority: &[u8], protocols: &[&[u8]]) -> tokio_rustls::TlsConnector {
    let mut config = client_config_builder()
        .with_root_certificates(trust_anchors(authority))
        .with_no_client_auth();
    config.alpn_protocols = protocols.iter().map(|protocol| protocol.to_vec()).collect();

    tokio_rustls::TlsConnector::from(std::sync::Arc::new(config))
}

/// A client configuration builder on the provider the server side names.
///
/// `ClientConfig::builder` resolves the process-level crypto provider from
/// crate features and panics when that is ambiguous, which is the whole of what
/// [`a_caller_installed_crypto_provider_is_the_one_build_runs_on`] covers on
/// the server side. The harness would panic the same way under the same graph,
/// so both ends go through the same named provider.
#[cfg(feature = "tls")]
fn client_config_builder() -> tokio_rustls::rustls::ConfigBuilder<
    tokio_rustls::rustls::ClientConfig,
    tokio_rustls::rustls::WantsVerifier,
> {
    tokio_rustls::rustls::ClientConfig::builder_with_provider(crate::server::tls::crypto_provider())
        .with_safe_default_protocol_versions()
        .expect("the named provider serves the default protocol versions")
}

/// The PEM authority in `certificate`, as a store a client can verify against.
///
/// Every TLS case here trusts one minted authority and differs only in what it
/// does afterwards -- offering a client certificate, offering an ALPN
/// identifier, or offering neither -- so the anchors are built once and the
/// difference is left at each call site.
#[cfg(feature = "tls")]
fn trust_anchors(certificate: &[u8]) -> tokio_rustls::rustls::RootCertStore {
    use tokio_rustls::rustls::{
        RootCertStore,
        pki_types::{CertificateDer, pem::PemObject as _},
    };

    let mut roots = RootCertStore::empty();
    for anchor in CertificateDer::pem_slice_iter(certificate) {
        roots
            .add(anchor.expect("CA certificate parses"))
            .expect("CA is a trust anchor");
    }

    roots
}

/// A service whose entire response is what the connection settled on.
///
/// The ALPN identifier the handshake agreed and the HTTP version the request
/// arrived with: the first is what the driver is pinned from and the second is
/// what it actually spoke, so the two disagreeing is a visible failure rather
/// than a served request.
#[cfg(feature = "tls")]
fn negotiated_protocol_service() -> crate::router::service::Service<()> {
    let document = kynos_openapi::Document::new(
        kynos_openapi::SpecVersion::V3_1,
        kynos_openapi::Info::new("Test", "1"),
    );
    crate::router::service::Service::new(document, |request: crate::http::Request| async move {
        use crate::extract::FromRequestParts as _;

        let (mut parts, _) = request.into_parts();
        let version = parts.version;
        let connection =
            crate::extract::connection::Connection::from_request_parts(&mut parts, &())
                .await
                .expect("extracting a connection is infallible");
        let alpn = connection.alpn_protocol().map_or_else(
            || "none".to_owned(),
            |alpn| String::from_utf8_lossy(alpn).into_owned(),
        );

        crate::http::Response::new(crate::http::body::Body::from_bytes(bytes::Bytes::from(
            format!("{alpn} {version:?}"),
        )))
    })
}

/// A TLS listener with `resumption`, optionally requiring a client certificate,
/// serving how many certificates the client presented.
///
/// `None` never calls the setter, so the listener runs on whatever
/// [`TlsConfig::from_pem`](crate::server::tls::TlsConfig::from_pem) chose.
///
/// The count is what a resumed session must still carry: a resumption that
/// dropped the verified identity would answer `0` where the full handshake
/// answered `1`.
#[cfg(all(feature = "tls", feature = "http1"))]
async fn resumption_server(
    resumption: Option<crate::server::tls::SessionResumption>,
    mutual: bool,
) -> (
    std::net::SocketAddr,
    Authority,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<crate::error::Result<()>>,
) {
    let issued = authority();
    let (address, shutdown_sender, server) = resumption_replica(&issued, resumption, mutual).await;

    (address, issued, shutdown_sender, server)
}

/// [`resumption_server`] under an identity the caller holds, so that several
/// listeners can present the same one — which is what makes them replicas to a
/// client rather than three different servers.
#[cfg(all(feature = "tls", feature = "http1"))]
async fn resumption_replica(
    issued: &Authority,
    resumption: Option<crate::server::tls::SessionResumption>,
    mutual: bool,
) -> (
    std::net::SocketAddr,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<crate::error::Result<()>>,
) {
    let mut tls = crate::server::tls::TlsConfig::from_pem(
        issued.server.certificate.as_bytes(),
        issued.server.key.as_bytes(),
    )
    .expect("server identity parses");
    if let Some(resumption) = resumption {
        tls = tls.session_resumption(resumption);
    }
    if mutual {
        tls = tls.require_client_certificate(
            crate::server::tls::ClientCertificateConfig::from_pem_roots(
                issued.certificate.as_bytes(),
            )
            .expect("CA parses"),
        );
    }
    let document = kynos_openapi::Document::new(
        kynos_openapi::SpecVersion::V3_1,
        kynos_openapi::Info::new("Test", "1"),
    );
    let service = crate::router::service::Service::<()>::new(
        document,
        |request: crate::http::Request| async move {
            use crate::extract::FromRequestParts as _;

            let (mut parts, _) = request.into_parts();
            let connection =
                crate::extract::connection::Connection::from_request_parts(&mut parts, &())
                    .await
                    .expect("extracting a connection is infallible");
            crate::http::Response::new(crate::http::body::Body::from_bytes(bytes::Bytes::from(
                connection.peer_certificates().len().to_string(),
            )))
        },
    );
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(service)
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .tls(tls)
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("TLS listener prepares");
    let address = bound.local_addrs()[0];

    (address, shutdown_sender, tokio::spawn(bound.serve()))
}

/// A client speaking only `version`, with a session store of its own, and the
/// issued client identity when `mutual`.
///
/// Each call is a distinct client: resumption state lives in the store, so two
/// connectors from two calls never resume each other's sessions.
#[cfg(all(feature = "tls", feature = "http1"))]
fn resuming_client(
    issued: &Authority,
    version: &'static tokio_rustls::rustls::SupportedProtocolVersion,
    mutual: bool,
) -> tokio_rustls::TlsConnector {
    use tokio_rustls::rustls::{
        ClientConfig,
        pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject as _},
    };

    let builder = ClientConfig::builder_with_provider(crate::server::tls::crypto_provider())
        .with_protocol_versions(&[version])
        .expect("the named provider serves both versions")
        .with_root_certificates(trust_anchors(issued.certificate.as_bytes()));
    let mut config = if mutual {
        let chain = CertificateDer::pem_slice_iter(issued.client.certificate.as_bytes())
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("client chain parses");
        let key =
            PrivateKeyDer::from_pem_slice(issued.client.key.as_bytes()).expect("client key parses");
        builder
            .with_client_auth_cert(chain, key)
            .expect("client identity is valid")
    } else {
        builder.with_no_client_auth()
    };
    config.alpn_protocols = vec![b"http/1.1".to_vec()];

    tokio_rustls::TlsConnector::from(std::sync::Arc::new(config))
}

/// One connection through `connector`: how its handshake went, and what the
/// server answered.
///
/// The response is read to its end because a TLS 1.3 server sends its tickets
/// after the handshake, and a client only stores what it has read.
#[cfg(all(feature = "tls", feature = "http1"))]
async fn connect(
    connector: &tokio_rustls::TlsConnector,
    address: std::net::SocketAddr,
) -> (tokio_rustls::rustls::HandshakeKind, String) {
    use http_body_util::{BodyExt as _, Empty};
    use hyper_util::rt::TokioIo;
    use tokio_rustls::rustls::pki_types::ServerName;

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let stream = connector
        .connect(
            ServerName::try_from("localhost").expect("valid DNS name"),
            stream,
        )
        .await
        .expect("TLS handshake succeeds");
    let kind = stream
        .get_ref()
        .1
        .handshake_kind()
        .expect("a completed handshake has a kind");
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("HTTP/1 handshake completes");
    let connection = tokio::spawn(connection);
    let request = hyper::Request::builder()
        .uri("/")
        .header(hyper::header::HOST, "localhost")
        .body(Empty::<bytes::Bytes>::new())
        .expect("request builds");
    let body = sender
        .send_request(request)
        .await
        .expect("request succeeds")
        .into_body()
        .collect()
        .await
        .expect("response body reads")
        .to_bytes();
    drop(sender);
    connection.abort();

    (kind, String::from_utf8_lossy(&body).into_owned())
}

/// By default a session is a ticket the client holds, so no number of other
/// clients evicts it.
///
/// It also holds that a returning client resumes by default at all, over
/// either version: its first connection is a full handshake and its return a
/// resumed one, which is everything a case asserting only that would.
///
/// This is what separates the default from rustls's own, so it builds its
/// config without the setter rather than naming a variant: reverting the
/// default to a cache fails it. In rustls's own, every session
/// was an entry in a cache of 256 — each TLS 1.3 handshake stores two and each
/// TLS 1.2 one stores one — so three hundred clients in between pushed the
/// first one's out and it paid a full handshake on return. With stateless
/// tickets the session travels with the client, so nothing it depends on is
/// stored to push. Over both versions, because the cache still exists under
/// tickets for a TLS 1.2 client that takes none, and resuming from it would
/// satisfy every other case here.
#[cfg(all(feature = "tls", feature = "http1"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_default_session_survives_any_number_of_other_clients() {
    use tokio_rustls::rustls::{HandshakeKind, version};

    let (address, issued, shutdown_sender, server) = resumption_server(None, false).await;

    for version in [&version::TLS13, &version::TLS12] {
        let returning = resuming_client(&issued, version, false);
        assert_eq!(connect(&returning, address).await.0, HandshakeKind::Full);
        for _ in 0..300 {
            connect(&resuming_client(&issued, version, false), address).await;
        }
        assert_eq!(
            connect(&returning, address).await.0,
            HandshakeKind::Resumed,
            "{version:?}: a default session is not evicted by other clients"
        );
    }

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// A bounded cache resumes what it holds and evicts what it cannot.
#[cfg(all(feature = "tls", feature = "http1"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_cache_resumes_what_it_holds_and_evicts_past_its_capacity() {
    use std::num::NonZeroUsize;

    use tokio_rustls::rustls::{HandshakeKind, version};

    use crate::server::tls::SessionResumption;

    let (address, issued, shutdown_sender, server) = resumption_server(
        Some(SessionResumption::Cache {
            capacity: NonZeroUsize::new(4).expect("four is non-zero"),
        }),
        false,
    )
    .await;

    for version in [&version::TLS13, &version::TLS12] {
        let client = resuming_client(&issued, version, false);
        connect(&client, address).await;
        assert_eq!(
            connect(&client, address).await.0,
            HandshakeKind::Resumed,
            "{version:?}"
        );

        let evicted = resuming_client(&issued, version, false);
        connect(&evicted, address).await;
        for _ in 0..50 {
            connect(&resuming_client(&issued, version, false), address).await;
        }
        assert_eq!(
            connect(&evicted, address).await.0,
            HandshakeKind::Full,
            "{version:?}: fifty clients past a capacity of four evict the first"
        );
    }

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

#[cfg(all(feature = "tls", feature = "http1"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disabled_resumption_pays_a_full_handshake_every_time() {
    use tokio_rustls::rustls::{HandshakeKind, version};

    use crate::server::tls::SessionResumption;

    let (address, issued, shutdown_sender, server) =
        resumption_server(Some(SessionResumption::Disabled), false).await;

    for version in [&version::TLS13, &version::TLS12] {
        let client = resuming_client(&issued, version, false);
        for _ in 0..2 {
            assert_eq!(
                connect(&client, address).await.0,
                HandshakeKind::Full,
                "{version:?}"
            );
        }
    }

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// A resumed session under mutual TLS still carries the certificate its full
/// handshake verified, so an operation reading the client's identity reads the
/// same one on either.
#[cfg(all(feature = "tls", feature = "http1"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_resumed_mutual_tls_session_keeps_the_client_identity() {
    use tokio_rustls::rustls::{HandshakeKind, version};

    let (address, issued, shutdown_sender, server) = resumption_server(None, true).await;

    for version in [&version::TLS13, &version::TLS12] {
        let client = resuming_client(&issued, version, true);
        assert_eq!(
            connect(&client, address).await,
            (HandshakeKind::Full, "1".to_owned()),
            "{version:?}"
        );
        assert_eq!(
            connect(&client, address).await,
            (HandshakeKind::Resumed, "1".to_owned()),
            "{version:?}"
        );
    }

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// Shared tickets under `secret`, each call deriving its keys afresh as a
/// separate process would.
#[cfg(all(feature = "tls", feature = "http1"))]
fn shared_tickets(secret: u8) -> crate::server::tls::SessionResumption {
    crate::server::tls::SessionResumption::SharedTickets {
        keys: crate::server::tls::ticket::TicketKeys::new(ticket_key(secret), []),
        lifetime: std::time::Duration::from_secs(3600),
    }
}

#[cfg(feature = "tls")]
fn ticket_key(secret: u8) -> crate::server::tls::ticket::TicketKey {
    crate::server::tls::ticket::TicketKey::from_secret(&[secret; 32])
        .expect("the named provider seals shared tickets")
}

/// Replicas given the same ticket secret resume one another's sessions, and a
/// server given another does not.
///
/// Each replica derives its keys separately, so nothing but the secret is
/// shared between them, and each has a session cache of its own that the other
/// never wrote to: a `Resumed` on the second replica can only have come from
/// opening the first one's ticket. Two clients, one per direction, because a
/// client that resumed on the second replica may then hold a ticket that
/// replica issued. Under mutual TLS, so the identity the first replica verified
/// is what the second reports.
#[cfg(all(feature = "tls", feature = "http1"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replicas_sharing_a_ticket_secret_resume_each_others_sessions() {
    use tokio_rustls::rustls::{HandshakeKind, version};

    let issued = authority();
    let (first, first_shutdown, first_server) =
        resumption_replica(&issued, Some(shared_tickets(1)), true).await;
    let (second, second_shutdown, second_server) =
        resumption_replica(&issued, Some(shared_tickets(1)), true).await;
    let (stranger, stranger_shutdown, stranger_server) =
        resumption_replica(&issued, Some(shared_tickets(2)), true).await;

    for version in [&version::TLS13, &version::TLS12] {
        for (issuer, resumer) in [(first, second), (second, first)] {
            let client = resuming_client(&issued, version, true);
            assert_eq!(
                connect(&client, issuer).await,
                (HandshakeKind::Full, "1".to_owned()),
                "{version:?}"
            );
            assert_eq!(
                connect(&client, resumer).await,
                (HandshakeKind::Resumed, "1".to_owned()),
                "{version:?}: a replica opens the ticket its peer sealed"
            );
        }

        let client = resuming_client(&issued, version, true);
        connect(&client, first).await;
        assert_eq!(
            connect(&client, stranger).await,
            (HandshakeKind::Full, "1".to_owned()),
            "{version:?}: a server under another secret cannot open the ticket"
        );
    }

    for (shutdown, server) in [
        (first_shutdown, first_server),
        (second_shutdown, second_server),
        (stranger_shutdown, stranger_server),
    ] {
        let _ = shutdown.send(());
        server
            .await
            .expect("server task joins")
            .expect("server exits cleanly");
    }
}

/// A shared ticketer over `keys`, honouring tickets for an hour.
#[cfg(feature = "tls")]
fn shared_ticketer(
    keys: crate::server::tls::ticket::TicketKeys,
) -> crate::server::tls::ticket::SharedTicketer {
    crate::server::tls::ticket::SharedTicketer::new(
        keys,
        std::time::Duration::from_secs(3600),
        &crate::server::tls::crypto_provider(),
    )
    .expect("an hour is a valid ticket lifetime")
}

/// The ticket construction `ticket.rs` documents, byte for byte, on both
/// providers: what lets replicas on different providers resume one another's
/// sessions is that the construction is fixed, so it is pinned rather than
/// round-tripped, which a construction that changed would also pass.
///
/// The expected ticket was computed outside Rust, from RFC 5869's HKDF over
/// SHA-384 and AES-256-GCM, for the secret `[7; 32]`, a salt of thirty-two
/// `0x5a` bytes standing in for the random one, the second 1,700,000,000 and
/// the session `kynos session`: the name expanded under the name label, then
/// the key and the IV expanded under the key label, the salt and `key` or
/// `iv`, sealing the session under the name, salt and second as associated
/// data.
#[cfg(feature = "tls")]
#[test]
fn a_shared_ticket_is_the_documented_construction_on_either_provider() {
    use tokio_rustls::rustls::crypto::{
        CryptoProvider, GetRandomFailed, SecureRandom, aws_lc_rs, ring,
    };

    use crate::server::tls::ticket::{SharedTicketer, TicketKey, TicketKeys};

    #[derive(Debug)]
    struct FixedSalt;

    impl SecureRandom for FixedSalt {
        fn fill(&self, buffer: &mut [u8]) -> Result<(), GetRandomFailed> {
            buffer.fill(0x5a);
            Ok(())
        }
    }

    const ISSUED: u64 = 1_700_000_000;
    const SESSION: &[u8] = b"kynos session";
    const NAME: &str = "c531bf1f7144eb2212f6a827bc52aafe";
    const TICKET: &str = concat!(
        "c531bf1f7144eb2212f6a827bc52aafe",
        "5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a",
        "000000006553f100",
        "ff07768047b4e0ab04f64f9167",
        "b58e64c4396dca0e61260881fd44f4e3",
    );

    let hex = |bytes: &[u8]| {
        use std::fmt::Write as _;

        bytes.iter().fold(String::new(), |mut hex, byte| {
            write!(hex, "{byte:02x}").expect("a String takes any write");
            hex
        })
    };
    for provider in [aws_lc_rs::default_provider(), ring::default_provider()] {
        let provider = CryptoProvider {
            secure_random: &FixedSalt,
            ..provider
        };
        let key = TicketKey::derive(&provider, &[7; 32]).expect("the provider seals tickets");
        assert_eq!(format!("{key:?}"), format!("TicketKey({NAME})"));

        let ticketer = SharedTicketer::new(
            TicketKeys::new(key, []),
            std::time::Duration::from_secs(3600),
            &provider,
        )
        .expect("an hour is a valid ticket lifetime");
        let ticket = ticketer
            .seal_at(ISSUED, SESSION)
            .expect("the session seals");
        assert_eq!(hex(&ticket), TICKET);
        assert_eq!(ticketer.open_at(ISSUED, &ticket).as_deref(), Some(SESSION));
    }
}

/// A ticket opens under its issuing key and under a key that is merely
/// accepted, from the same secret wherever it was derived, and under no other.
#[cfg(feature = "tls")]
#[test]
fn a_shared_ticket_opens_under_its_secret_and_no_other() {
    use crate::server::tls::ticket::TicketKeys;

    let issuer = shared_ticketer(TicketKeys::new(ticket_key(1), []));
    let ticket = issuer.seal_at(100, b"session").expect("a ticket seals");
    let again = issuer.seal_at(100, b"session").expect("a ticket seals");

    assert_ne!(ticket, again, "no two tickets share a salt");
    assert!(
        !ticket.windows(7).any(|window| window == b"session"),
        "the session is not carried in the clear"
    );
    for (keys, opens) in [
        (TicketKeys::new(ticket_key(1), []), true),
        (TicketKeys::new(ticket_key(2), [ticket_key(1)]), true),
        (TicketKeys::new(ticket_key(2), []), false),
        (TicketKeys::new(ticket_key(2), [ticket_key(3)]), false),
    ] {
        assert_eq!(
            shared_ticketer(keys.clone()).open_at(100, &ticket),
            opens.then(|| b"session".to_vec()),
            "{keys:?}"
        );
    }
}

/// No ticket that differs from an issued one opens: every single-byte change
/// and every truncation is refused, across the key name, the salt, the issue
/// time, the ciphertext and the tag.
///
/// The issue time is the field this matters most for, since it is sent in the
/// clear and decides whether the ticket is still honoured.
#[cfg(feature = "tls")]
#[test]
fn a_shared_ticket_that_was_altered_does_not_open() {
    let ticketer = shared_ticketer(crate::server::tls::ticket::TicketKeys::new(
        ticket_key(1),
        [],
    ));
    let ticket = ticketer.seal_at(100, b"session").expect("a ticket seals");

    for index in 0..ticket.len() {
        let mut altered = ticket.clone();
        altered[index] ^= 1;
        assert_eq!(ticketer.open_at(100, &altered), None, "byte {index}");
        assert_eq!(
            ticketer.open_at(100, &ticket[..index]),
            None,
            "truncated to {index}"
        );
    }
    let mut extended = ticket.clone();
    extended.push(0);
    assert_eq!(ticketer.open_at(100, &extended), None);
    assert_eq!(ticketer.open_at(100, &ticket), Some(b"session".to_vec()));
}

/// A ticket is honoured through its lifetime and not a second past it, and one
/// a replica with a faster clock issued is honoured rather than refused.
#[cfg(feature = "tls")]
#[test]
fn a_shared_ticket_is_honoured_for_its_lifetime_only() {
    let ticketer = shared_ticketer(crate::server::tls::ticket::TicketKeys::new(
        ticket_key(1),
        [],
    ));
    let ticket = ticketer
        .seal_at(10_000, b"session")
        .expect("a ticket seals");

    for (now, opens) in [
        (10_000, true),
        (13_600, true),
        (13_601, false),
        (9_000, true),
        (u64::MAX, false),
    ] {
        assert_eq!(
            ticketer.open_at(now, &ticket),
            opens.then(|| b"session".to_vec()),
            "at {now}"
        );
    }
    assert_eq!(
        tokio_rustls::rustls::server::ProducesTickets::lifetime(&ticketer),
        3600,
        "clients are told the lifetime tickets are held to"
    );
}

/// What rustls calls stamps and judges a ticket by the wall clock, in seconds
/// since the Unix epoch: the one clock replicas agree on. A ticketer on any
/// other clock would agree with itself and with no replica.
#[cfg(feature = "tls")]
#[test]
fn a_shared_ticket_is_stamped_and_judged_by_the_wall_clock() {
    use tokio_rustls::rustls::server::ProducesTickets;

    let wall_clock = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is past the epoch")
            .as_secs()
    };
    let ticketer = shared_ticketer(crate::server::tls::ticket::TicketKeys::new(
        ticket_key(1),
        [],
    ));

    let before = wall_clock();
    let issued = ticketer.encrypt(b"session").expect("a ticket seals");
    let after = wall_clock();
    assert_eq!(
        ticketer.open_at(after, &issued),
        Some(b"session".to_vec()),
        "issued no earlier than a lifetime ago"
    );
    assert_eq!(
        ticketer.open_at(after + 3601, &issued),
        None,
        "issued no later than now"
    );

    let fresh = ticketer
        .seal_at(before, b"session")
        .expect("a ticket seals");
    let stale = ticketer
        .seal_at(before - 3601, b"session")
        .expect("a ticket seals");
    assert_eq!(ticketer.decrypt(&fresh), Some(b"session".to_vec()));
    assert_eq!(
        ticketer.decrypt(&stale),
        None,
        "judged at no earlier than now"
    );
}

/// Rotating through a handle reaches a ticketer already built from a clone of
/// it: the two-step rotation keeps a ticket resuming until its key is dropped,
/// and issues under the new key from the step that names it.
#[cfg(feature = "tls")]
#[test]
fn rotating_shared_ticket_keys_reaches_a_running_ticketer() {
    use crate::server::tls::ticket::TicketKeys;

    let keys = TicketKeys::new(ticket_key(1), []);
    let ticketer = shared_ticketer(keys.clone());
    let only_new = shared_ticketer(TicketKeys::new(ticket_key(2), []));
    let old = ticketer.seal_at(100, b"old").expect("a ticket seals");

    keys.rotate(ticket_key(1), [ticket_key(2)]);
    let still_old = ticketer.seal_at(100, b"still").expect("a ticket seals");
    assert_eq!(
        only_new.open_at(100, &still_old),
        None,
        "accepting a key does not issue under it"
    );

    keys.rotate(ticket_key(2), [ticket_key(1)]);
    let new = ticketer.seal_at(100, b"new").expect("a ticket seals");
    assert_eq!(only_new.open_at(100, &new), Some(b"new".to_vec()));
    assert_eq!(ticketer.open_at(100, &old), Some(b"old".to_vec()));

    keys.rotate(ticket_key(2), []);
    assert_eq!(
        ticketer.open_at(100, &old),
        None,
        "a dropped key opens nothing"
    );
    assert_eq!(ticketer.open_at(100, &new), Some(b"new".to_vec()));
}

/// The two ways configuring shared tickets fails, each as its own variant: a
/// lifetime outside one second to seven days, at both edges, and a provider
/// with no AES-256-GCM to seal with.
#[cfg(feature = "tls")]
#[test]
fn shared_tickets_reject_an_unusable_lifetime_and_an_unusable_provider() {
    use std::time::Duration;

    use crate::server::tls::{error::TlsError, ticket::TicketKey};

    const WEEK: Duration = Duration::from_secs(7 * 24 * 60 * 60);

    let identity = server_identity();
    for (lifetime, valid) in [
        (Duration::ZERO, false),
        (Duration::from_millis(999), false),
        (Duration::from_secs(1), true),
        (WEEK, true),
        (WEEK + Duration::from_nanos(1), false),
        (Duration::MAX, false),
    ] {
        let built = crate::server::tls::TlsConfig::from_pem(
            identity.certificate.as_bytes(),
            identity.key.as_bytes(),
        )
        .expect("server identity parses")
        .session_resumption(crate::server::tls::SessionResumption::SharedTickets {
            keys: crate::server::tls::ticket::TicketKeys::new(ticket_key(1), []),
            lifetime,
        })
        .build();
        match built {
            Ok(_) => assert!(valid, "{lifetime:?} was accepted"),
            Err(TlsError::TicketLifetime(reported)) => {
                assert!(!valid, "{lifetime:?} was refused");
                assert_eq!(reported, lifetime);
            }
            Err(other) => panic!("{lifetime:?}: {other}"),
        }
    }

    let provider = crate::server::tls::crypto_provider();
    let without_aes_256 = tokio_rustls::rustls::crypto::CryptoProvider {
        cipher_suites: provider
            .cipher_suites
            .iter()
            .copied()
            .filter(|suite| {
                suite.suite() != tokio_rustls::rustls::CipherSuite::TLS13_AES_256_GCM_SHA384
            })
            .collect(),
        ..(*provider).clone()
    };
    assert!(matches!(
        TicketKey::derive(&without_aes_256, &[1; 32]),
        Err(TlsError::TicketCipher)
    ));
}

#[cfg(feature = "tls")]
#[test]
fn tls_rejects_empty_pem_and_zero_handshake_timeouts() {
    let identity = server_identity();

    assert!(matches!(
        crate::server::tls::TlsConfig::from_pem(b"", b""),
        Err(crate::server::tls::error::TlsError::EmptyPem { .. })
    ));

    let config = crate::server::tls::TlsConfig::from_pem(
        identity.certificate.as_bytes(),
        identity.key.as_bytes(),
    )
    .expect("server identity parses");
    assert!(matches!(
        config.handshake_timeout(std::time::Duration::ZERO),
        Err(crate::server::tls::error::TlsError::ZeroHandshakeTimeout)
    ));

    let client = crate::server::tls::ClientCertificateConfig::from_pem_roots(
        identity.certificate.as_bytes(),
    )
    .expect("certificate parses as a trust anchor");
    assert!(matches!(
        client.with_pem_crls(b""),
        Err(crate::server::tls::error::TlsError::EmptyPem { .. })
    ));
}

/// A malformed PEM is a rustls failure Kynos wraps, and the wrapper says only
/// which material was expected. Without the cause, "invalid certificate PEM" is
/// the whole diagnostic and the reader learns nothing about what the parser
/// actually objected to.
#[cfg(feature = "tls")]
#[test]
fn a_malformed_pem_keeps_its_parser_failure_as_a_cause() {
    let error =
        crate::server::tls::TlsConfig::from_pem(b"-----BEGIN CERTIFICATE-----\nnot base64", b"")
            .expect_err("a truncated certificate does not parse");

    assert!(matches!(
        error,
        crate::server::tls::error::TlsError::Pem { .. }
    ));
    assert!(
        std::error::Error::source(&error).is_some(),
        "the parser failure must survive as a cause, not as a formatted string"
    );
}

/// Each malformed PEM names the material it was read for: the private key
/// beside a certificate, and a revocation list beside client roots.
#[cfg(feature = "tls")]
#[test]
fn a_malformed_pem_names_the_material_it_was_read_for() {
    use crate::server::tls::error::TlsError;

    let identity = server_identity();

    assert!(matches!(
        crate::server::tls::TlsConfig::from_pem(
            identity.certificate.as_bytes(),
            b"-----BEGIN PRIVATE KEY-----\nnot base64",
        ),
        Err(TlsError::Pem {
            kind: "private key",
            ..
        })
    ));

    let client = crate::server::tls::ClientCertificateConfig::from_pem_roots(
        identity.certificate.as_bytes(),
    )
    .expect("certificate parses as a trust anchor");
    assert!(matches!(
        client.with_pem_crls(b"-----BEGIN X509 CRL-----\nnot base64"),
        Err(TlsError::Pem {
            kind: "certificate revocation list",
            ..
        })
    ));
}

/// A private key that is not the certificate's own is refused when the
/// runtime is built, for the default certificate and for one selected by SNI.
#[cfg(feature = "tls")]
#[test]
fn a_private_key_that_does_not_match_its_certificate_is_refused() {
    use crate::server::tls::{TlsConfig, error::TlsError};

    let (certified, other) = (server_identity(), server_identity());

    let error = TlsConfig::from_pem(certified.certificate.as_bytes(), other.key.as_bytes())
        .expect("both halves parse")
        .build()
        .expect_err("a mismatched default key is refused");
    assert!(matches!(error, TlsError::PrivateKey(_)), "{error:?}");

    let error = TlsConfig::from_pem(certified.certificate.as_bytes(), certified.key.as_bytes())
        .expect("server identity parses")
        .with_server_certificate(
            ["example.com"],
            certified.certificate.as_bytes(),
            other.key.as_bytes(),
        )
        .expect("both halves parse")
        .build()
        .expect_err("a mismatched SNI key is refused");
    assert!(matches!(error, TlsError::PrivateKey(_)), "{error:?}");
}

/// Client-certificate material that parses as PEM but cannot verify anyone
/// is refused when the runtime is built: a root whose DER is not a
/// certificate, and a revocation list whose DER is not one either. Both keep
/// rustls's account as their cause.
#[cfg(feature = "tls")]
#[test]
fn client_verification_material_that_cannot_verify_is_refused() {
    use crate::server::tls::{ClientCertificateConfig, TlsConfig, error::TlsError};

    let identity = server_identity();
    let build = |client: ClientCertificateConfig| {
        TlsConfig::from_pem(identity.certificate.as_bytes(), identity.key.as_bytes())
            .expect("server identity parses")
            .require_client_certificate(client)
            .build()
    };

    let root = ClientCertificateConfig::from_pem_roots(
        b"-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n",
    )
    .expect("well-formed PEM parses");
    let crl = ClientCertificateConfig::from_pem_roots(identity.certificate.as_bytes())
        .expect("certificate parses as a trust anchor")
        .with_pem_crls(b"-----BEGIN X509 CRL-----\nAAAA\n-----END X509 CRL-----\n")
        .expect("well-formed PEM parses");
    for (case, client) in [("root", root), ("revocation list", crl)] {
        let error = build(client).expect_err("unusable material is refused");
        assert!(
            matches!(error, TlsError::ClientVerifier(_)),
            "{case}: {error:?}"
        );
        assert!(
            std::error::Error::source(&error).is_some(),
            "{case}: rustls's account survives as a cause"
        );
    }
}

#[cfg(feature = "tls")]
#[test]
fn tls_rejects_repeated_sni_names() {
    let identity = server_identity();

    let config = crate::server::tls::TlsConfig::from_pem(
        identity.certificate.as_bytes(),
        identity.key.as_bytes(),
    )
    .expect("server identity parses");
    assert!(matches!(
        config.with_server_certificate(
            ["EXAMPLE.COM", "example.com"],
            identity.certificate.as_bytes(),
            identity.key.as_bytes(),
        ),
        Err(crate::server::tls::error::TlsError::ServerName(name)) if name == "example.com"
    ));
}

#[cfg(feature = "tls")]
fn sni_refusal(
    earlier: &[&str],
    names: &[&str],
) -> std::result::Result<crate::server::tls::TlsConfig, crate::server::tls::error::TlsError> {
    let identity = server_identity();
    let mut config = crate::server::tls::TlsConfig::from_pem(
        identity.certificate.as_bytes(),
        identity.key.as_bytes(),
    )
    .expect("server identity parses");
    if !earlier.is_empty() {
        config = config
            .with_server_certificate(
                earlier.iter().copied(),
                identity.certificate.as_bytes(),
                identity.key.as_bytes(),
            )
            .expect("the earlier names register");
    }
    config.with_server_certificate(
        names.iter().copied(),
        identity.certificate.as_bytes(),
        identity.key.as_bytes(),
    )
}

#[cfg(feature = "tls")]
#[test]
fn tls_rejects_an_empty_sni_name_list() {
    assert!(matches!(
        sni_refusal(&[], &[]),
        Err(crate::server::tls::error::TlsError::ServerName(name)) if name.is_empty()
    ));
}

#[cfg(feature = "tls")]
#[test]
fn tls_rejects_an_empty_sni_name() {
    assert!(matches!(
        sni_refusal(&[], &["example.com", ""]),
        Err(crate::server::tls::error::TlsError::ServerName(name)) if name.is_empty()
    ));
}

/// Registered names are compared after lowercasing, like the names a call
/// repeats within itself.
#[cfg(feature = "tls")]
#[test]
fn tls_rejects_an_sni_name_an_earlier_call_registered() {
    assert!(matches!(
        sni_refusal(&["example.com"], &["Example.COM"]),
        Err(crate::server::tls::error::TlsError::ServerName(name)) if name == "example.com"
    ));
    assert!(
        matches!(
            sni_refusal(&["example.com"], &["example.org", "example.com"]),
            Err(crate::server::tls::error::TlsError::ServerName(name)) if name == "example.com"
        ),
        "the refusal names the registered name, not the call's first"
    );
    assert!(
        sni_refusal(&["example.com"], &["example.org"]).is_ok(),
        "a name no earlier call registered is accepted"
    );
}

#[cfg(feature = "tls")]
#[test]
fn tls_rejects_an_sni_name_that_is_not_a_server_name() {
    assert!(matches!(
        sni_refusal(&[], &["example.com", "not a host"]),
        Err(crate::server::tls::error::TlsError::ServerName(name)) if name == "not a host"
    ));
}

/// rustls never selects a certificate by an IP literal, which RFC 6066 §3
/// forbids as an SNI `HostName`, so such a certificate could never be served.
#[cfg(feature = "tls")]
#[test]
fn tls_rejects_an_ip_literal_sni_name() {
    for literal in ["192.0.2.1", "2001:db8::1"] {
        assert!(
            matches!(
                sni_refusal(&[], &["example.com", literal]),
                Err(crate::server::tls::error::TlsError::ServerName(name)) if name == literal
            ),
            "{literal} must be refused as an SNI name"
        );
    }
}

/// `TlsConfig::build` with no crypto provider installed anywhere.
///
/// rustls resolves the process-level provider from the `aws-lc-rs` and `ring`
/// features of whatever `rustls` the graph unified on, and *panics* when zero
/// or two of them are compiled in. Cargo features are additive, so a dependency
/// that wants `ring` for its own reasons puts the whole graph in that state and
/// no downstream manifest can leave it. The provider Kynos builds on therefore
/// has to be one Kynos names, and this is the case saying that a build with
/// nothing installed still reaches it.
#[cfg(feature = "tls")]
#[test]
fn tls_builds_on_a_provider_kynos_names_rather_than_one_it_resolves() {
    assert!(
        tokio_rustls::rustls::crypto::CryptoProvider::get_default().is_none(),
        "the premise of this case is that nothing installed a default provider"
    );

    let identity = server_identity();

    crate::server::tls::TlsConfig::from_pem(
        identity.certificate.as_bytes(),
        identity.key.as_bytes(),
    )
    .expect("server identity parses")
    .build()
    .expect("a TLS runtime builds with no provider installed by anyone");

    assert!(
        tokio_rustls::rustls::crypto::CryptoProvider::get_default().is_none(),
        "naming a provider must not install one: the process default is the binary's to set"
    );
}

/// The same two claims, on the path a client certificate adds.
///
/// `require_client_certificate` puts a second rustls constructor in `build` --
/// the client verifier's -- and rustls resolves *its* provider the same
/// implicit way, so a fix that reaches only the `ServerConfig` half leaves the
/// panic on the mutual-TLS path and starts writing the process-wide static
/// there. The install half needs no ambiguous graph to see, which is why it is
/// what this case asserts: after an mutual-TLS `build`, an application that
/// calls `install_default` with its own FIPS or hardware-backed provider must
/// still win, and it cannot if Kynos got there first.
#[cfg(feature = "tls")]
#[test]
fn a_mutual_tls_build_installs_no_process_wide_provider() {
    assert!(
        tokio_rustls::rustls::crypto::CryptoProvider::get_default().is_none(),
        "the premise of this case is that nothing installed a default provider"
    );

    let issued = authority();
    let client_authentication =
        crate::server::tls::ClientCertificateConfig::from_pem_roots(issued.certificate.as_bytes())
            .expect("CA parses");

    crate::server::tls::TlsConfig::from_pem(
        issued.server.certificate.as_bytes(),
        issued.server.key.as_bytes(),
    )
    .expect("server identity parses")
    .require_client_certificate(client_authentication)
    .build()
    .expect("a mutual-TLS runtime builds with no provider installed by anyone");

    assert!(
        tokio_rustls::rustls::crypto::CryptoProvider::get_default().is_none(),
        "configuring client-certificate verification must not install a process default either"
    );
}

/// A *usable* caller-installed provider is the one that serves.
///
/// The negative case below shows a caller's provider being consulted by
/// refusing to build on it, which says nothing about a server that starts. This
/// is the positive half, and the two are not the same claim: "a FIPS or
/// hardware-backed provider still wins" is a promise about traffic, so what
/// holds it has to be traffic.
///
/// The discriminator is the cipher suite. Both providers list
/// `TLS13_AES_256_GCM_SHA384` first, so a server on the installed provider --
/// restricted to `ChaCha20` and nothing else -- settles on a suite a server on
/// Kynos's own `aws-lc-rs` would not have chosen, against a client whose offer
/// is deliberately left wide so the intersection is the server's restriction
/// alone. `ring` rather than a doctored `aws-lc-rs` because it is a different
/// provider, which is the situation being claimed.
#[cfg(all(feature = "tls", feature = "http1"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_usable_caller_installed_provider_is_the_one_that_serves() {
    use tokio_rustls::rustls::{
        CipherSuite, ClientConfig,
        crypto::{CryptoProvider, ring},
        pki_types::ServerName,
    };

    CryptoProvider {
        cipher_suites: vec![ring::cipher_suite::TLS13_CHACHA20_POLY1305_SHA256],
        ..ring::default_provider()
    }
    .install_default()
    .expect("no other test in this process installed a provider");

    let (address, authority, shutdown_sender, server) = tls_server(test_service()).await;

    let mut client =
        ClientConfig::builder_with_provider(std::sync::Arc::new(ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("ring serves the default protocol versions")
            .with_root_certificates(trust_anchors(authority.as_bytes()))
            .with_no_client_auth();
    client.alpn_protocols = vec![b"http/1.1".to_vec()];

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let stream = tokio_rustls::TlsConnector::from(std::sync::Arc::new(client))
        .connect(
            ServerName::try_from("localhost").expect("valid DNS name"),
            stream,
        )
        .await
        .expect("the handshake completes on the installed provider");

    let negotiated = stream
        .get_ref()
        .1
        .negotiated_cipher_suite()
        .expect("a completed handshake settled a cipher suite");
    assert_eq!(
        negotiated.suite(),
        CipherSuite::TLS13_CHACHA20_POLY1305_SHA256,
        "the server served on the provider the caller installed, not on Kynos's own"
    );

    drop(stream);
    let _ = shutdown_sender.send(());
    server.await.expect("server task joins").expect("serves");
}

/// A provider a caller installed is the one `build` runs on, and a provider
/// that can serve nothing is reported rather than panicked on.
///
/// Both halves are one case because one provider shows them: a provider with no
/// cipher suites is usable for nothing, so a `build` that succeeds cannot have
/// consulted it, and a `build` that panics has not reported it. rustls's own
/// `ServerConfig::builder` does the second -- it unwraps the protocol-version
/// check -- from inside a function whose signature already carries a
/// `TlsError`.
///
/// `install_default` writes a process-wide static that accepts one write. That
/// is shared state only within a process, and nextest gives each test its own,
/// so this case observes a default nothing else in the suite can have touched.
/// It rests on the property `tests/hermeticity.rs` holds the runner to rather
/// than making an exception to it.
#[cfg(feature = "tls")]
#[test]
fn a_caller_installed_crypto_provider_is_the_one_build_runs_on() {
    let unusable = tokio_rustls::rustls::crypto::CryptoProvider {
        cipher_suites: Vec::new(),
        ..tokio_rustls::rustls::crypto::aws_lc_rs::default_provider()
    };
    unusable
        .install_default()
        .expect("no other test in this process installed a provider");

    let identity = server_identity();
    let error = crate::server::tls::TlsConfig::from_pem(
        identity.certificate.as_bytes(),
        identity.key.as_bytes(),
    )
    .expect("server identity parses")
    .build()
    .expect_err("a provider that serves nothing cannot build a TLS runtime");

    assert!(
        matches!(
            error,
            crate::server::tls::error::TlsError::CryptoProvider(_)
        ),
        "an unusable provider is its own failure, not a certificate one: {error}"
    );
    assert!(
        std::error::Error::source(&error).is_some(),
        "rustls's account of why the provider is unusable must survive as a cause"
    );
}

/// A PEM certificate and the key that signs for it.
///
/// Minted here rather than committed. A published archive is immutable, so a
/// private key that reaches one reaches it permanently -- and the argument
/// `examples/tls.rs` makes for itself applies unchanged to a test: a transport
/// case that mints its own material runs with nothing prepared, and has no PEM
/// to expire.
#[cfg(feature = "tls")]
struct Identity {
    certificate: String,
    key: String,
}

/// A self-signed server identity for `localhost`, trusted by nothing.
///
/// Enough for every case that only has to parse an identity or configure one.
#[cfg(feature = "tls")]
fn server_identity() -> Identity {
    let certified = rcgen::generate_simple_self_signed(["localhost".to_owned()])
        .expect("a self-signed certificate");

    Identity {
        certificate: certified.cert.pem(),
        key: certified.signing_key.serialize_pem(),
    }
}

/// A certificate authority and the two identities it issues.
///
/// One authority signs both ends, because that is what the mutual case needs:
/// the client verifies the server against this trust anchor and the server
/// verifies the client against the same one.
#[cfg(feature = "tls")]
struct Authority {
    /// The trust anchor itself, which both ends are given.
    certificate: String,
    server: Identity,
    client: Identity,
}

#[cfg(feature = "tls")]
fn authority() -> Authority {
    authority_issuing(&[]).0
}

/// [`authority`], also issuing one server identity per entry of `servers`,
/// each for the names that entry lists.
#[cfg(feature = "tls")]
fn authority_issuing(servers: &[&[&str]]) -> (Authority, Vec<Identity>) {
    use rcgen::{
        BasicConstraints, CertificateParams, CertifiedIssuer, DnType, ExtendedKeyUsagePurpose,
        IsCa, KeyPair, KeyUsagePurpose,
    };

    let mut root = CertificateParams::new(Vec::new()).expect("no subject alternative names");
    root.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    root.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    root.distinguished_name
        .push(DnType::CommonName, "Kynos test authority");
    let issuer = CertifiedIssuer::self_signed(root, KeyPair::generate().expect("a key pair"))
        .expect("a self-signed authority");

    // A distinct common name per identity, so no leaf shares a subject with the
    // authority that issued it and chain building has one answer.
    let leaf = |names: Vec<String>, common: &str, purpose: ExtendedKeyUsagePurpose| {
        let mut params = CertificateParams::new(names).expect("usable subject alternative names");
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![purpose];
        params
            .distinguished_name
            .push(DnType::CommonName, common.to_owned());

        let key = KeyPair::generate().expect("a key pair");
        let certificate = params
            .signed_by(&key, &issuer)
            .expect("an issued certificate");

        Identity {
            certificate: certificate.pem(),
            key: key.serialize_pem(),
        }
    };

    let authority = Authority {
        certificate: issuer.pem(),
        server: leaf(
            vec!["localhost".to_owned()],
            "Kynos test server",
            ExtendedKeyUsagePurpose::ServerAuth,
        ),
        client: leaf(
            vec!["client.example.test".to_owned()],
            "Kynos test client",
            ExtendedKeyUsagePurpose::ClientAuth,
        ),
    };
    let servers = servers
        .iter()
        .enumerate()
        .map(|(index, names)| {
            leaf(
                names.iter().map(|name| (*name).to_owned()).collect(),
                &format!("Kynos test server {index}"),
                ExtendedKeyUsagePurpose::ServerAuth,
            )
        })
        .collect();
    (authority, servers)
}

fn test_service() -> crate::router::service::Service<()> {
    let document = kynos_openapi::Document::new(
        kynos_openapi::SpecVersion::V3_1,
        kynos_openapi::Info::new("Test", "1"),
    );
    crate::router::service::Service::new(document, |_| async {
        crate::http::Response::new(crate::http::body::Body::from_bytes(
            bytes::Bytes::from_static(b"ok"),
        ))
    })
}

fn blocking_service() -> (
    crate::router::service::Service<()>,
    std::sync::Arc<tokio::sync::Notify>,
    std::sync::Arc<tokio::sync::Notify>,
) {
    let started = std::sync::Arc::new(tokio::sync::Notify::new());
    let release = std::sync::Arc::new(tokio::sync::Notify::new());
    let service = {
        let started = std::sync::Arc::clone(&started);
        let release = std::sync::Arc::clone(&release);
        let document = kynos_openapi::Document::new(
            kynos_openapi::SpecVersion::V3_1,
            kynos_openapi::Info::new("Test", "1"),
        );
        crate::router::service::Service::new(document, move |_| {
            let started = std::sync::Arc::clone(&started);
            let release = std::sync::Arc::clone(&release);
            async move {
                started.notify_one();
                release.notified().await;
                crate::http::Response::new(crate::http::body::Body::from_bytes(
                    bytes::Bytes::from_static(b"ok"),
                ))
            }
        })
    };
    (service, started, release)
}

/// A service whose first response body sends nothing until the returned
/// sender fires, and whose later responses are the usual `ok`.
#[cfg(feature = "http2")]
fn held_body_service() -> (
    crate::router::service::Service<()>,
    tokio::sync::oneshot::Sender<()>,
) {
    /// `ok`, once `0` resolves.
    struct Held(Option<tokio::sync::oneshot::Receiver<()>>);

    impl http_body::Body for Held {
        type Data = bytes::Bytes;
        type Error = crate::http::body::BoxError;

        fn poll_frame(
            mut self: std::pin::Pin<&mut Self>,
            context: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
            let Some(held) = self.0.as_mut() else {
                return std::task::Poll::Ready(None);
            };
            // A dropped sender releases the body as a sent one does.
            let _ = std::task::ready!(std::future::Future::poll(std::pin::Pin::new(held), context));
            self.0 = None;
            std::task::Poll::Ready(Some(Ok(http_body::Frame::data(bytes::Bytes::from_static(
                b"ok",
            )))))
        }
    }

    let (release, held) = tokio::sync::oneshot::channel();
    let held = std::sync::Arc::new(std::sync::Mutex::new(Some(held)));
    let document = kynos_openapi::Document::new(
        kynos_openapi::SpecVersion::V3_1,
        kynos_openapi::Info::new("Test", "1"),
    );
    let service = crate::router::service::Service::new(document, move |_| {
        let held = held.lock().expect("the held receiver's lock").take();
        async move {
            let body = match held {
                Some(held) => crate::http::body::Body::from_body(Held(Some(held))),
                None => crate::http::body::Body::from_bytes(bytes::Bytes::from_static(b"ok")),
            };
            crate::http::Response::new(body)
        }
    });
    (service, release)
}

#[cfg(feature = "http1")]
fn request_http1(address: std::net::SocketAddr) -> String {
    use std::io::{Read as _, Write as _};

    let mut stream = std::net::TcpStream::connect(address).expect("server accepts");
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("request writes");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("response reads");
    response
}

/// Every configuration `validate_protocol_config` refuses.
///
/// One row per refused condition rather than per `return`: several conditions
/// share one refusal, and a condition dropped from an `||` chain reads as a
/// branch that still has a case. A limit that stops being checked is one hyper
/// is handed instead -- where a zero window stalls a connection and an
/// oversized send buffer does not fit the protocol field it is written to.
///
/// Gated on both protocols because the function takes one argument per enabled
/// protocol; the feature matrix builds combinations with only one, and there
/// the signature is a different one.
#[cfg(all(feature = "http1", feature = "http2"))]
mod protocol_configuration {
    use std::time::Duration;

    use crate::server::{
        error::ServerError,
        protocol::{
            http1::Http1Config,
            http2::{Http2Config, Http2FlowControl, Http2KeepAlive},
            validate_protocol_config,
        },
    };

    fn refused(http1: Http1Config, http2: Http2Config) -> String {
        match validate_protocol_config(http1, http2) {
            Err(ServerError::InvalidConfiguration(reason)) => reason.to_owned(),
            Err(other) => panic!("expected an invalid configuration, got {other}"),
            Ok(()) => panic!("this configuration must be refused"),
        }
    }

    /// One row per condition: what was set, and what it must be told.
    #[expect(clippy::too_many_lines)]
    fn cases() -> Vec<(&'static str, Http1Config, Http2Config, &'static str)> {
        vec![
            (
                "no room for a single header",
                Http1Config::default().max_headers(0),
                Http2Config::default(),
                "HTTP/1 max_headers must be non-zero",
            ),
            (
                "a read buffer under the floor",
                Http1Config::default().max_buffer_size(8_191),
                Http2Config::default(),
                "HTTP/1 max_buffer_size must be at least 8192",
            ),
            (
                "a header read timeout that expires at once",
                Http1Config::default().header_read_timeout(Some(Duration::ZERO)),
                Http2Config::default(),
                "HTTP/1 header_read_timeout must be non-zero when enabled",
            ),
            (
                "no stream may be opened",
                Http1Config::default(),
                Http2Config::default().max_concurrent_streams(0),
                "HTTP/2 limits must be non-zero and fit their protocol fields",
            ),
            (
                "no room for a single header",
                Http1Config::default(),
                Http2Config::default().max_header_list_size(0),
                "HTTP/2 limits must be non-zero and fit their protocol fields",
            ),
            (
                "a send buffer that holds nothing",
                Http1Config::default(),
                Http2Config::default().max_send_buffer_size(0),
                "HTTP/2 limits must be non-zero and fit their protocol fields",
            ),
            (
                "a send buffer larger than the field that carries it",
                Http1Config::default(),
                Http2Config::default().max_send_buffer_size(u32::MAX as usize + 1),
                "HTTP/2 limits must be non-zero and fit their protocol fields",
            ),
            (
                "no reset stream may await acceptance",
                Http1Config::default(),
                Http2Config::default().max_pending_accept_reset_streams(0),
                "HTTP/2 limits must be non-zero and fit their protocol fields",
            ),
            (
                "no stream may be reset for a local error",
                Http1Config::default(),
                Http2Config::default().max_local_error_reset_streams(0),
                "HTTP/2 limits must be non-zero and fit their protocol fields",
            ),
            (
                "a fixed stream window that admits nothing",
                Http1Config::default(),
                Http2Config::default().flow_control(Http2FlowControl::Fixed {
                    initial_stream_window_size: 0,
                    initial_connection_window_size: 1024,
                }),
                "HTTP/2 fixed flow-control windows must be non-zero",
            ),
            (
                "a fixed connection window that admits nothing",
                Http1Config::default(),
                Http2Config::default().flow_control(Http2FlowControl::Fixed {
                    initial_stream_window_size: 1024,
                    initial_connection_window_size: 0,
                }),
                "HTTP/2 fixed flow-control windows must be non-zero",
            ),
            (
                "a fixed stream window past the protocol ceiling",
                Http1Config::default(),
                Http2Config::default().flow_control(Http2FlowControl::Fixed {
                    initial_stream_window_size: 1 << 31,
                    initial_connection_window_size: 1024,
                }),
                "HTTP/2 fixed flow-control windows must not exceed 2147483647",
            ),
            (
                "a fixed connection window past the protocol ceiling",
                Http1Config::default(),
                Http2Config::default().flow_control(Http2FlowControl::Fixed {
                    initial_stream_window_size: 1024,
                    initial_connection_window_size: 1 << 31,
                }),
                "HTTP/2 fixed flow-control windows must not exceed 2147483647",
            ),
            (
                "a keep-alive that never waits",
                Http1Config::default(),
                Http2Config::default().keep_alive(Some(Http2KeepAlive::new(
                    Duration::ZERO,
                    Duration::from_secs(5),
                ))),
                "HTTP/2 keep-alive durations must be non-zero",
            ),
            (
                "a keep-alive that never waits for its acknowledgement",
                Http1Config::default(),
                Http2Config::default().keep_alive(Some(Http2KeepAlive::new(
                    Duration::from_secs(5),
                    Duration::ZERO,
                ))),
                "HTTP/2 keep-alive durations must be non-zero",
            ),
            (
                "an idle timeout that expires at once",
                Http1Config::default(),
                Http2Config::default().idle_timeout(Some(Duration::ZERO)),
                "HTTP/2 idle_timeout must be non-zero when enabled",
            ),
        ]
    }

    #[test]
    fn each_case_is_refused_for_the_reason_it_names() {
        for (description, http1, http2, expected) in cases() {
            assert_eq!(refused(http1, http2), expected, "{description}");
        }
    }

    #[test]
    fn the_defaults_are_accepted() {
        validate_protocol_config(Http1Config::default(), Http2Config::default())
            .expect("the defaults Kynos ships must be a configuration it accepts");
    }

    /// The case table refuses one byte under the floor; this accepts the floor
    /// itself, so the refusal cannot creep up to include it.
    #[test]
    fn a_read_buffer_of_exactly_the_floor_is_accepted() {
        use crate::server::protocol::http1::MIN_HTTP1_BUFFER_SIZE;

        validate_protocol_config(
            Http1Config::default().max_buffer_size(MIN_HTTP1_BUFFER_SIZE),
            Http2Config::default(),
        )
        .expect("a read buffer of exactly the floor is one hyper accepts");
    }

    /// The case table refuses a send buffer one past `u32::MAX`; this accepts
    /// `u32::MAX` itself, the largest the protocol field carries, so the
    /// refusal cannot creep down to include it.
    #[test]
    fn a_send_buffer_of_exactly_the_field_width_is_accepted() {
        validate_protocol_config(
            Http1Config::default(),
            Http2Config::default().max_send_buffer_size(u32::MAX as usize),
        )
        .expect("a send buffer of exactly u32::MAX fits the field that carries it");
    }

    /// RFC 9113 §6.9.1 caps a flow-control window at 2^31-1, and `h2` asserts
    /// it during the handshake, so a larger window panicked every HTTP/2
    /// connection instead of failing `prepare`. The case table refuses one
    /// past the ceiling on either window; this holds the ceiling itself to
    /// acceptance, and the far end of `u32` to refusal.
    #[test]
    fn fixed_windows_are_held_to_the_protocol_ceiling_on_either_side() {
        const CEILING: u32 = (1 << 31) - 1;
        let fixed = |stream, connection| {
            Http2Config::default().flow_control(Http2FlowControl::Fixed {
                initial_stream_window_size: stream,
                initial_connection_window_size: connection,
            })
        };

        validate_protocol_config(Http1Config::default(), fixed(CEILING, CEILING))
            .expect("a window of exactly 2^31-1 is one the protocol carries");
        assert_eq!(
            refused(Http1Config::default(), fixed(CEILING, u32::MAX)),
            "HTTP/2 fixed flow-control windows must not exceed 2147483647",
        );
    }

    /// A count of conditions, so a limit added without a case fails the
    /// build, whether as a refusal of its own or as one more `||` on a shared
    /// one: each refusal is one condition, and each `||` adds another.
    #[test]
    fn every_refusal_has_a_case() {
        const SOURCE: &str = include_str!("protocol.rs");

        let code = SOURCE
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let conditions =
            code.matches("ServerError::InvalidConfiguration(").count() + code.matches("||").count();
        assert_eq!(
            cases().len(),
            conditions,
            "`protocol.rs` refuses {conditions} condition(s) and {} have a case",
            cases().len()
        );
    }
}

/// Every [`InvalidConfiguration`](crate::server::error::ServerError::InvalidConfiguration)
/// the crate constructs, as the file under `src/` that constructs it and its
/// message.
///
/// Each message is asserted exactly by the case that reaches it: the
/// `protocol_configuration` table, the TCP keepalive and request-body idle
/// timeout cases, the mutual-TLS case here, and `tests/serve_io.rs`.
const CONFIGURATION_REFUSALS: &[(&str, &str)] = &[
    (
        "server/request_body.rs",
        "request_body_idle_timeout must be non-zero when enabled",
    ),
    (
        "server/mod.rs",
        "mutual TLS adds itself to the document, so no serve_io connection may still share the service",
    ),
    (
        "server/mod.rs",
        "serve_io serves plaintext, so TLS must not be configured",
    ),
    ("server/protocol.rs", "HTTP/1 max_headers must be non-zero"),
    (
        "server/protocol.rs",
        "HTTP/1 max_buffer_size must be at least 8192",
    ),
    (
        "server/protocol.rs",
        "HTTP/1 header_read_timeout must be non-zero when enabled",
    ),
    (
        "server/protocol.rs",
        "HTTP/2 limits must be non-zero and fit their protocol fields",
    ),
    (
        "server/protocol.rs",
        "HTTP/2 fixed flow-control windows must be non-zero",
    ),
    (
        "server/protocol.rs",
        "HTTP/2 fixed flow-control windows must not exceed 2147483647",
    ),
    (
        "server/protocol.rs",
        "HTTP/2 keep-alive durations must be non-zero",
    ),
    (
        "server/protocol.rs",
        "HTTP/2 idle_timeout must be non-zero when enabled",
    ),
    (
        "server/tcp.rs",
        "TCP keepalive durations must be between 1 and 32767 seconds",
    ),
];

/// The configuration refusals in `src/`, read from the source, are exactly
/// [`CONFIGURATION_REFUSALS`].
///
/// Read crate-wide rather than per file, so a refusal added in a module no
/// count covered fails here instead of going unasserted; and by message, so
/// one reworded without its case moving fails too. Test files are skipped, as
/// are comment lines, so a doc example cannot count.
#[test]
fn every_configuration_refusal_in_the_crate_is_listed() {
    const CONSTRUCTOR: &str = "ServerError::InvalidConfiguration(";

    fn sources(directory: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(directory).expect("a readable source directory") {
            let path = entry.expect("a readable directory entry").path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name != "tests") {
                    sources(&path, found);
                }
            } else if path.extension().is_some_and(|extension| extension == "rs")
                && path.file_name().is_some_and(|name| name != "tests.rs")
            {
                found.push(path);
            }
        }
    }

    let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
    let mut files = Vec::new();
    sources(root, &mut files);

    let mut constructed = Vec::new();
    for file in files {
        let source = std::fs::read_to_string(&file).expect("a readable source file");
        let code = source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let relative = file
            .strip_prefix(root)
            .expect("a file under `src/`")
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");

        for (at, _) in code.match_indices(CONSTRUCTOR) {
            let message = code[at + CONSTRUCTOR.len()..]
                .split('"')
                .nth(1)
                .expect("a refusal's message is a string literal");
            constructed.push((relative.clone(), message.to_owned()));
        }
    }
    constructed.sort();

    let mut listed = CONFIGURATION_REFUSALS
        .iter()
        .map(|(file, message)| ((*file).to_owned(), (*message).to_owned()))
        .collect::<Vec<_>>();
    listed.sort();

    assert_eq!(
        constructed, listed,
        "the configuration refusals `src/` constructs and the ones listed here differ"
    );
}

/// An accept loop that ended without returning is its own failure, not an
/// invalid setting, and the error says which way it ended.
#[tokio::test]
async fn a_failed_accept_loop_is_reported_as_one() {
    use crate::server::{accept_loop_failure, error::ServerError};

    let mut loops = tokio::task::JoinSet::new();
    loops.spawn(async { panic!("an accept loop panicking on purpose") });
    let panicked = loops
        .join_next()
        .await
        .expect("one loop was spawned")
        .expect_err("the loop panicked");

    loops.spawn(std::future::pending::<()>());
    loops.abort_all();
    let cancelled = loops
        .join_next()
        .await
        .expect("one loop was spawned")
        .expect_err("the loop was cancelled");

    for (error, expected_panicked, message) in [
        (panicked, true, "an accept loop panicked"),
        (cancelled, false, "an accept loop was cancelled"),
    ] {
        let failure = accept_loop_failure(&error);
        assert_eq!(failure.to_string(), message);
        assert!(
            matches!(failure, ServerError::AcceptLoop { panicked } if panicked == expected_panicked),
            "{failure:?}"
        );
    }
}

/// A served request carries the address it arrived from.
///
/// [`ConnectInfo`](crate::extract::connection::ConnectInfo) documents that the
/// server inserts one before handing a request over, and its extractor
/// `expect`s exactly that. Nothing did: `serve_http` inserted a private
/// `ConnectionMetadata` instead, so every handler taking a `ConnectInfo`
/// panicked on every request under the real server —
/// [`examples/parameters.rs`](../../examples/parameters.rs) among them, where
/// it is presented as working code.
///
/// The assertion is on the *value* rather than on mere presence, because an
/// address the server invented would satisfy presence and still be wrong.
#[cfg(feature = "http1")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_served_request_carries_the_address_it_arrived_from() {
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(peer_address_service())
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());

    let (client_address, response) =
        tokio::task::spawn_blocking(move || request_http1_from(address))
            .await
            .expect("blocking client joins");

    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let reported = response
        .rsplit("\r\n\r\n")
        .next()
        .expect("a response has a body")
        .to_owned();
    assert_eq!(
        reported,
        client_address.to_string(),
        "the handler was told `{reported}` and the client connected from `{client_address}`"
    );

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// A service whose entire response is the peer address it was handed.
///
/// Reports `absent` rather than panicking, so a failure reads as a comparison
/// against the address the client actually used instead of as a dropped
/// connection.
#[cfg(feature = "http1")]
fn peer_address_service() -> crate::router::service::Service<()> {
    let document = kynos_openapi::Document::new(
        kynos_openapi::SpecVersion::V3_1,
        kynos_openapi::Info::new("Test", "1"),
    );
    crate::router::service::Service::new(document, |request: crate::http::Request| async move {
        // Through the extractor rather than through the extension it happens to
        // read, so the test cannot pass while `ConnectInfo` is broken.
        use crate::extract::FromRequestParts as _;

        let (mut parts, _) = request.into_parts();
        let crate::extract::connection::ConnectInfo(peer) =
            crate::extract::connection::ConnectInfo::from_request_parts(&mut parts, &())
                .await
                .expect("extracting a peer address is infallible");
        crate::http::Response::new(crate::http::body::Body::from_bytes(bytes::Bytes::from(
            peer.to_string(),
        )))
    })
}

/// Requests over HTTP/1, reporting the address the client connected from.
///
/// The peer address the server sees is this socket's local address, which is
/// what makes the comparison in the caller a real one rather than a round trip
/// through a value the server chose.
#[cfg(feature = "http1")]
fn request_http1_from(address: std::net::SocketAddr) -> (std::net::SocketAddr, String) {
    use std::io::{Read as _, Write as _};

    let mut stream = std::net::TcpStream::connect(address).expect("server accepts");
    let client_address = stream.local_addr().expect("a connected socket has one");
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("request writes");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("response reads");
    (client_address, response)
}

/// The HTTP/1 header cap holds on the wire: a request head carrying exactly
/// the configured number of fields is served, and one more is answered 431
/// (RFC 6585 section 5).
///
/// Over a real socket, because what is owed is the driver refusing the head,
/// not a value handed to it. The default cap and a configured one, each at
/// the boundary, so neither a cap that stopped reaching the driver nor one
/// off by one passes.
#[cfg(feature = "http1")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_request_head_past_the_http1_header_cap_is_answered_431() {
    for (http1, cap) in [
        (Http1Config::default(), 100),
        (Http1Config::default().max_headers(8), 8),
    ] {
        let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
        let bound = crate::server::Server::new(test_service())
            .http1(http1)
            .bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
                let _ = shutdown_receiver.await;
            }))
            .prepare()
            .await
            .expect("loopback listener binds");
        let address = bound.local_addrs()[0];
        let server = tokio::spawn(bound.serve());

        let (at_cap, past_cap) = tokio::task::spawn_blocking(move || {
            (
                request_http1_with_fields(address, cap),
                request_http1_with_fields(address, cap + 1),
            )
        })
        .await
        .expect("blocking client joins");

        assert!(
            at_cap.starts_with("HTTP/1.1 200 OK"),
            "{cap} field(s) under a cap of {cap}: {at_cap}"
        );
        assert!(
            past_cap.starts_with("HTTP/1.1 431 "),
            "{} field(s) under a cap of {cap}: {past_cap}",
            cap + 1
        );

        let _ = shutdown_sender.send(());
        server
            .await
            .expect("server task joins")
            .expect("server exits cleanly");
    }
}

/// A plaintext server over [`test_service`], configured by `configure`.
#[cfg(any(feature = "http1", feature = "http2"))]
async fn configured_server(
    configure: impl FnOnce(crate::server::Server<()>) -> crate::server::Server<()>,
) -> (
    std::net::SocketAddr,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<crate::error::Result<()>>,
) {
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = configure(crate::server::Server::new(test_service()))
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    (address, shutdown_sender, tokio::spawn(bound.serve()))
}

/// `keep_alive(false)` reaches the driver: the response says `close`, and the
/// server closes after it. A client that asked for nothing is what tells this
/// apart from honouring the client's own `close`; with keep-alive on, the
/// read below would wait for a close that never came.
#[cfg(feature = "http1")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_http1_server_without_keep_alive_closes_after_one_response() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let (address, shutdown_sender, server) =
        configured_server(|server| server.http1(Http1Config::default().keep_alive(false))).await;

    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .expect("the request writes");
    let mut response = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_to_end(&mut response),
    )
    .await
    .expect("the server closes the connection")
    .expect("the response reads");
    let response = String::from_utf8(response)
        .expect("the response is text")
        .to_ascii_lowercase();

    assert!(response.starts_with("http/1.1 200 ok\r\n"), "{response}");
    assert!(response.contains("\r\nconnection: close\r\n"), "{response}");

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// `max_buffer_size` reaches the driver: a head of four fields filling the
/// 8 KiB floor is answered 431 under that floor, while a longer one is served
/// under the default.
///
/// The refused head is exactly 8 KiB and unterminated, so the driver has read
/// every byte when it refuses: unread bytes at its close would reset the
/// connection, and a reset may discard the 431 before the client reads it.
#[cfg(feature = "http1")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_request_head_past_the_http1_buffer_is_answered_431() {
    use std::io::{Read as _, Write as _};

    const PREFIX: &str = "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nX-Padding: ";

    for (http1, head, status) in [
        (
            Http1Config::default(),
            format!("{PREFIX}{}\r\n\r\n", "a".repeat(12 * 1024)),
            "HTTP/1.1 200 ",
        ),
        (
            Http1Config::default().max_buffer_size(8_192),
            format!("{PREFIX}{}", "a".repeat(8_192 - PREFIX.len())),
            "HTTP/1.1 431 ",
        ),
    ] {
        let (address, shutdown_sender, server) =
            configured_server(|server| server.http1(http1)).await;

        let response = tokio::task::spawn_blocking(move || {
            let mut stream = std::net::TcpStream::connect(address).expect("server accepts");
            stream.write_all(head.as_bytes()).expect("request writes");
            let mut response = String::new();
            stream
                .read_to_string(&mut response)
                .expect("response reads");
            response
        })
        .await
        .expect("blocking client joins");

        assert!(response.starts_with(status), "{http1:?}: {response}");

        let _ = shutdown_sender.send(());
        server
            .await
            .expect("server task joins")
            .expect("server exits cleanly");
    }
}

/// What a server announces at the start of an HTTP/2 connection, read off the
/// frames it sends before it answers the client's first PING.
#[cfg(feature = "http2")]
#[derive(Debug, PartialEq, Eq)]
struct Announced {
    /// The server's SETTINGS frame, by identifier (RFC 9113 §6.5.2).
    settings: std::collections::BTreeMap<u16, u32>,
    /// The connection window: RFC 9113 §6.9.2's 65,535 plus every
    /// `WINDOW_UPDATE` the server sent on stream 0.
    connection_window: u32,
}

/// Opens a prior-knowledge HTTP/2 connection to `address` and reads what the
/// server announces, until it answers a PING sent once it acknowledged the
/// client's empty SETTINGS: the driver writes what it queued at the start of
/// the connection before it answers the PING.
#[cfg(feature = "http2")]
async fn announced(address: std::net::SocketAddr) -> Announced {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    stream
        .write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n\0\0\0\x04\0\0\0\0\0")
        .await
        .expect("the preface and an empty SETTINGS write");

    let mut announced = Announced {
        settings: std::collections::BTreeMap::new(),
        connection_window: 65_535,
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let mut header = [0_u8; 9];
            stream
                .read_exact(&mut header)
                .await
                .expect("a frame header reads");
            let length = u32::from_be_bytes([0, header[0], header[1], header[2]]);
            let (kind, flags) = (header[3], header[4]);
            let stream_id = u32::from_be_bytes([header[5], header[6], header[7], header[8]]);
            let mut payload = vec![0_u8; usize::try_from(length).expect("a frame fits")];
            stream
                .read_exact(&mut payload)
                .await
                .expect("a frame payload reads");
            match (kind, flags & 0x1, stream_id) {
                // SETTINGS with ACK: the server has read the client's.
                (0x4, 0x1, 0) => stream
                    .write_all(b"\0\0\x08\x06\0\0\0\0\0kynos-ok")
                    .await
                    .expect("a PING writes"),
                // The PING's ACK.
                (0x6, 0x1, 0) => return,
                (0x4, 0x0, 0) => {
                    for entry in payload.chunks_exact(6) {
                        announced.settings.insert(
                            u16::from_be_bytes([entry[0], entry[1]]),
                            u32::from_be_bytes([entry[2], entry[3], entry[4], entry[5]]),
                        );
                    }
                }
                (0x8, _, 0) => {
                    let increment =
                        u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                    announced.connection_window += increment & 0x7fff_ffff;
                }
                _ => {}
            }
        }
    })
    .await
    .expect("the server acknowledges the client's SETTINGS and its PING");
    announced
}

/// The HTTP/2 settings that reach a client do so as configured: the stream
/// cap, both fixed windows and the header-list cap, under the defaults and
/// under values that differ from every one of them. An adaptive window starts
/// from the protocol's own 65,535 on both sides and grows from there, so the
/// fixed windows it replaces reach no frame.
///
/// The send buffer and the two reset-stream caps announce nothing, so no
/// frame can show them reaching the driver; they are left to the setter and
/// validation cases.
#[cfg(feature = "http2")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_http2_server_announces_the_settings_it_was_configured_with() {
    const MAX_CONCURRENT_STREAMS: u16 = 0x3;
    const INITIAL_WINDOW_SIZE: u16 = 0x4;
    const MAX_HEADER_LIST_SIZE: u16 = 0x6;

    let fixed = Http2Config::default()
        .max_concurrent_streams(64)
        .flow_control(Http2FlowControl::Fixed {
            initial_stream_window_size: 256 * 1024,
            initial_connection_window_size: 512 * 1024,
        })
        .max_header_list_size(8 * 1024);
    let adaptive = Http2Config::default().flow_control(Http2FlowControl::Adaptive);

    for (http2, settings, connection_window) in [
        (
            Http2Config::default(),
            vec![
                (MAX_CONCURRENT_STREAMS, 200),
                (INITIAL_WINDOW_SIZE, 1024 * 1024),
                (MAX_HEADER_LIST_SIZE, 16 * 1024),
            ],
            1024 * 1024,
        ),
        (
            fixed,
            vec![
                (MAX_CONCURRENT_STREAMS, 64),
                (INITIAL_WINDOW_SIZE, 256 * 1024),
                (MAX_HEADER_LIST_SIZE, 8 * 1024),
            ],
            512 * 1024,
        ),
        (
            adaptive,
            vec![
                (MAX_CONCURRENT_STREAMS, 200),
                (INITIAL_WINDOW_SIZE, 65_535),
                (MAX_HEADER_LIST_SIZE, 16 * 1024),
            ],
            65_535,
        ),
    ] {
        let (address, shutdown_sender, server) =
            configured_server(|server| server.http2(http2)).await;

        let mut announced = announced(address).await;
        // Only what Kynos configures; the frame size, say, is hyper's own.
        announced.settings.retain(|identifier, _| {
            [
                MAX_CONCURRENT_STREAMS,
                INITIAL_WINDOW_SIZE,
                MAX_HEADER_LIST_SIZE,
            ]
            .contains(identifier)
        });
        assert_eq!(
            announced,
            Announced {
                settings: settings.into_iter().collect(),
                connection_window,
            },
            "{http2:?}"
        );

        let _ = shutdown_sender.send(());
        server
            .await
            .expect("server task joins")
            .expect("server exits cleanly");
    }
}

/// Requests over HTTP/1 with a head of exactly `fields` header fields, `Host`
/// and `Connection: close` among them.
#[cfg(feature = "http1")]
fn request_http1_with_fields(address: std::net::SocketAddr, fields: usize) -> String {
    use std::{
        fmt::Write as _,
        io::{Read as _, Write as _},
    };

    let mut head = String::from("GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n");
    for field in 2..fields {
        write!(head, "X-Field-{field}: {field}\r\n").expect("a String takes any write");
    }
    head.push_str("\r\n");

    let mut stream = std::net::TcpStream::connect(address).expect("server accepts");
    stream.write_all(head.as_bytes()).expect("request writes");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("response reads");
    response
}

/// The HTTP/2 header-list cap holds on the wire: a request whose header list
/// is past the cap is answered 431 on its stream, and one under it is served
/// on the same connection.
///
/// The default cap and a configured one. A list of 8 KiB is under the first
/// and past the second, so a configured cap that stopped reaching the driver
/// is served where it must be refused.
#[cfg(feature = "http2")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_header_list_past_the_http2_cap_is_answered_431() {
    use http_body_util::Empty;
    use hyper_util::rt::{TokioExecutor, TokioIo};

    for (http2, past, under) in [
        (Http2Config::default(), 20 * 1024, 8 * 1024),
        (
            Http2Config::default().max_header_list_size(4 * 1024),
            8 * 1024,
            1024,
        ),
    ] {
        let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
        let bound = crate::server::Server::new(test_service())
            .http2(http2)
            .bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
                let _ = shutdown_receiver.await;
            }))
            .prepare()
            .await
            .expect("loopback listener binds");
        let address = bound.local_addrs()[0];
        let server = tokio::spawn(bound.serve());

        let stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("server accepts");
        let (mut sender, connection) =
            hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
                .await
                .expect("HTTP/2 handshake completes");
        let connection = tokio::spawn(connection);

        let mut statuses = Vec::new();
        for length in [past, under] {
            let request = hyper::Request::builder()
                .uri("http://localhost/")
                .header("x-padding", "a".repeat(length))
                .body(Empty::<bytes::Bytes>::new())
                .expect("request builds");
            let response = sender
                .send_request(request)
                .await
                .expect("the server answers the stream");
            statuses.push(response.status());
        }
        assert_eq!(
            statuses,
            [
                crate::http::StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
                crate::http::StatusCode::OK
            ],
            "{http2:?}"
        );

        drop(sender);
        connection
            .await
            .expect("client connection task joins")
            .expect("client connection closes cleanly");
        let _ = shutdown_sender.send(());
        server
            .await
            .expect("server task joins")
            .expect("server exits cleanly");
    }
}

/// The idle timeout on a request body: a client that stops sending part-way
/// through a body held the connection, its permit and the handler task for as
/// long as it kept the socket open, because nothing below the handler bounded
/// the wait for the next frame.
mod request_body_idle {
    use std::time::Duration;

    use crate::{
        server::request_body::{
            DEFAULT_REQUEST_BODY_IDLE_TIMEOUT, validate_request_body_idle_timeout,
        },
        server::{Server, error::ServerError},
    };

    /// The idle timeout the stalling cases configure: short, so a passing case
    /// is quick, and far below [`STALL_BOUND`].
    #[cfg(any(feature = "http1", feature = "http2"))]
    const IDLE: Duration = Duration::from_millis(200);

    /// How long a stalling case waits for the server's answer.
    #[cfg(any(feature = "http1", feature = "http2"))]
    const STALL_BOUND: Duration = Duration::from_secs(5);

    /// The body a stalling client declares, of which it sends only `abc`.
    #[cfg(feature = "http1")]
    const STALLED_HEAD: &[u8] =
        b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 10\r\n\r\nabc";

    #[test]
    fn a_server_bounds_a_request_body_by_default() {
        assert_eq!(DEFAULT_REQUEST_BODY_IDLE_TIMEOUT, Duration::from_secs(30));
        assert_eq!(
            Server::new(super::test_service()).request_body_idle_timeout,
            Some(DEFAULT_REQUEST_BODY_IDLE_TIMEOUT),
            "a server bounds a stalled body unless told not to"
        );
    }

    #[test]
    fn a_timeout_that_expires_at_once_is_refused() {
        assert!(matches!(
            validate_request_body_idle_timeout(Some(Duration::ZERO)),
            Err(ServerError::InvalidConfiguration(
                "request_body_idle_timeout must be non-zero when enabled"
            ))
        ));
        validate_request_body_idle_timeout(None).expect("waiting indefinitely is a decision");
        validate_request_body_idle_timeout(Some(Duration::from_nanos(1)))
            .expect("any positive limit is accepted");
    }

    /// `prepare` refuses what the validator refuses, rather than binding with it.
    #[tokio::test]
    async fn prepare_refuses_a_timeout_that_expires_at_once() {
        let error = Server::new(super::test_service())
            .request_body_idle_timeout(Some(Duration::ZERO))
            .bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .prepare()
            .await
            .expect_err("a zero idle timeout prevents preparation");
        assert!(matches!(
            error,
            crate::Error::Server(ServerError::InvalidConfiguration(
                "request_body_idle_timeout must be non-zero when enabled"
            ))
        ));
    }

    /// A limit past the end of the clock -- `Duration::MAX` -- leaves the body
    /// waiting rather than panicking in `Instant + Duration`, and never marks
    /// the request stalled.
    #[tokio::test]
    async fn a_limit_past_the_clock_leaves_the_body_waiting() {
        use std::{
            pin::pin,
            task::{Context, Waker},
        };

        use http_body::Body as _;

        use crate::{
            http::{StatusCode, Version},
            server::request_body::{answer, bounded},
        };

        let (body, stall) = bounded(Silent, Some(Duration::MAX), Version::HTTP_11, |_| {
            unreachable!("a body that is not over is bounded")
        });
        let mut body = pin!(body);
        let mut context = Context::from_waker(Waker::noop());
        for _ in 0..2 {
            assert!(
                body.as_mut().poll_frame(&mut context).is_pending(),
                "a limit the clock cannot reach never fires"
            );
        }

        let response = answer(crate::http::Response::default(), stall);
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "a body that only waited is not a stall"
        );
    }

    /// The bound body reports its inner body's end and size, so a reader that
    /// sizes a buffer or stops early sees the body it would have unbounded.
    #[test]
    fn a_bounded_body_reports_its_inner_end_and_size() {
        use std::{
            pin::pin,
            task::{Context, Waker},
        };

        use bytes::Bytes;
        use http_body::Body as _;
        use http_body_util::Full;

        use crate::{http::Version, server::request_body::bounded};

        let (body, _) = bounded(
            Full::new(Bytes::from_static(b"abc")),
            Some(IDLE_UNREACHED),
            Version::HTTP_11,
            |_| unreachable!("a body that is not over is bounded"),
        );
        let mut body = pin!(body);
        assert!(
            !body.is_end_stream(),
            "a body with a frame left is not over"
        );
        assert_eq!(body.size_hint().exact(), Some(3));

        let mut context = Context::from_waker(Waker::noop());
        let frame = body.as_mut().poll_frame(&mut context);
        assert!(
            matches!(frame, std::task::Poll::Ready(Some(Ok(_)))),
            "the inner frame passes through"
        );
        assert!(
            body.is_end_stream(),
            "a body whose inner body ended is over"
        );
        assert_eq!(body.size_hint().exact(), Some(0));
    }

    /// A body that stalls ends with an error naming the limit it passed, and
    /// its request is answered 408 -- closing an HTTP/1 connection, and
    /// carrying no `Connection` field on HTTP/2, which hyper would strip before
    /// a client could see it.
    #[tokio::test]
    async fn a_stalled_body_names_its_limit_and_closes_only_http1() {
        use http_body_util::BodyExt as _;

        use crate::{
            http::{HeaderValue, StatusCode, Version, header},
            server::request_body::{answer, bounded},
        };

        let limit = Duration::from_millis(1);
        for (version, connection) in [
            (Version::HTTP_10, Some(HeaderValue::from_static("close"))),
            (Version::HTTP_11, Some(HeaderValue::from_static("close"))),
            (Version::HTTP_2, None),
        ] {
            let (mut body, stall) = bounded(Silent, Some(limit), version, |_| {
                unreachable!("a body that is not over is bounded")
            });
            let error = tokio::time::timeout(IDLE_UNREACHED, body.frame())
                .await
                .expect("the limit fires before the bound")
                .expect("a stall ends the body with a frame")
                .expect_err("the frame is the stall");
            assert_eq!(
                error.to_string(),
                "no request body frame arrived within 1ms"
            );

            let response = answer(crate::http::Response::default(), stall);
            assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
            assert_eq!(
                response.headers().get(header::CONNECTION),
                connection.as_ref(),
                "{version:?}"
            );
        }
    }

    /// A body whose peer never sends another frame.
    struct Silent;

    impl http_body::Body for Silent {
        type Data = bytes::Bytes;
        type Error = std::convert::Infallible;

        fn poll_frame(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
            std::task::Poll::Pending
        }
    }

    /// A limit no unit test waits out.
    const IDLE_UNREACHED: Duration = Duration::from_secs(60);

    /// A service that reads its whole body, as every buffering codec does, and
    /// answers with its length -- or 400 when the read failed, which is what
    /// the codecs make of a failed read and what a stall must not reach the
    /// client as.
    #[cfg(any(feature = "http1", feature = "http2"))]
    fn reading_service() -> crate::router::service::Service<()> {
        use http_body_util::BodyExt as _;

        let document = kynos_openapi::Document::new(
            kynos_openapi::SpecVersion::V3_1,
            kynos_openapi::Info::new("Test", "1"),
        );
        crate::router::service::Service::new(document, |request: crate::http::Request| async move {
            let (status, body) = match request.into_body().collect().await {
                Ok(collected) => (
                    crate::http::StatusCode::OK,
                    collected.to_bytes().len().to_string(),
                ),
                Err(error) => (crate::http::StatusCode::BAD_REQUEST, error.to_string()),
            };
            let mut response =
                crate::http::Response::new(crate::http::body::Body::from_bytes(body.into()));
            *response.status_mut() = status;
            response
        })
    }

    /// A plaintext server over [`reading_service`] with `idle` as its limit.
    #[cfg(any(feature = "http1", feature = "http2"))]
    async fn idle_timed_server(
        idle: Option<Duration>,
    ) -> (
        std::net::SocketAddr,
        tokio::sync::oneshot::Sender<()>,
        tokio::task::JoinHandle<crate::error::Result<()>>,
    ) {
        idle_timed_server_over(reading_service(), idle).await
    }

    /// [`idle_timed_server`] over `service`.
    #[cfg(any(feature = "http1", feature = "http2"))]
    async fn idle_timed_server_over(
        service: crate::router::service::Service<()>,
        idle: Option<Duration>,
    ) -> (
        std::net::SocketAddr,
        tokio::sync::oneshot::Sender<()>,
        tokio::task::JoinHandle<crate::error::Result<()>>,
    ) {
        let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
        let bound = Server::new(service)
            .request_body_idle_timeout(idle)
            .bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
                let _ = shutdown_receiver.await;
            }))
            .prepare()
            .await
            .expect("loopback listener binds");
        let address = bound.local_addrs()[0];
        (address, shutdown_sender, tokio::spawn(bound.serve()))
    }

    /// Everything the server sends before it closes, failing past
    /// [`STALL_BOUND`].
    #[cfg(feature = "http1")]
    async fn read_until_closed(stream: &mut tokio::net::TcpStream) -> String {
        use tokio::io::AsyncReadExt as _;

        let mut response = Vec::new();
        tokio::time::timeout(STALL_BOUND, stream.read_to_end(&mut response))
            .await
            .expect("the server answers and closes within the bound")
            .expect("the response reads");
        String::from_utf8(response).expect("the response is text")
    }

    /// RFC 9110 §15.5.9: a request the server stopped waiting for is a 408,
    /// and the `close` option keeps the client from framing what it sends next
    /// as a new request -- so the server also lets go of the socket.
    #[cfg(feature = "http1")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_http1_body_that_stalls_is_answered_408_and_the_connection_closed() {
        use tokio::io::AsyncWriteExt as _;

        let (address, shutdown_sender, server) = idle_timed_server(Some(IDLE)).await;

        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("server accepts");
        stream
            .write_all(STALLED_HEAD)
            .await
            .expect("request writes");
        let response = read_until_closed(&mut stream).await.to_ascii_lowercase();

        assert!(
            response.starts_with("http/1.1 408 request timeout\r\n"),
            "{response}"
        );
        assert!(response.contains("\r\nconnection: close\r\n"), "{response}");
        assert!(
            response.contains("\r\ncontent-type: application/problem+json\r\n"),
            "{response}"
        );
        assert!(
            response.contains(r#""detail":"the request body sent nothing for 200ms""#),
            "the 408 names the limit the body passed: {response}"
        );

        let _ = shutdown_sender.send(());
        server
            .await
            .expect("server task joins")
            .expect("server exits cleanly");
    }

    /// A service that waits `delay` before it reads its body, says on
    /// `reading` when it starts, and answers as [`reading_service`] does.
    #[cfg(feature = "http1")]
    fn late_reading_service(
        delay: Duration,
        reading: std::sync::Arc<tokio::sync::Notify>,
    ) -> crate::router::service::Service<()> {
        use http_body_util::BodyExt as _;

        let document = kynos_openapi::Document::new(
            kynos_openapi::SpecVersion::V3_1,
            kynos_openapi::Info::new("Test", "1"),
        );
        crate::router::service::Service::new(document, move |request: crate::http::Request| {
            let reading = std::sync::Arc::clone(&reading);
            async move {
                tokio::time::sleep(delay).await;
                reading.notify_one();
                let (status, body) = match request.into_body().collect().await {
                    Ok(collected) => (
                        crate::http::StatusCode::OK,
                        collected.to_bytes().len().to_string(),
                    ),
                    Err(error) => (crate::http::StatusCode::BAD_REQUEST, error.to_string()),
                };
                let mut response =
                    crate::http::Response::new(crate::http::body::Body::from_bytes(body.into()));
                *response.status_mut() = status;
                response
            }
        })
    }

    /// The clock starts when the handler reads, not when the request arrives:
    /// a handler that waits three limits before reading, and then waits half
    /// of one for the rest of the body, reads it whole. Counted from arrival,
    /// the limit would long have passed.
    #[cfg(feature = "http1")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_handler_that_reads_late_is_not_charged_for_the_wait() {
        use tokio::io::AsyncWriteExt as _;

        const LIMIT: Duration = Duration::from_millis(400);

        let reading = std::sync::Arc::new(tokio::sync::Notify::new());
        let service = late_reading_service(LIMIT * 3, std::sync::Arc::clone(&reading));
        let (address, shutdown_sender, server) = idle_timed_server_over(service, Some(LIMIT)).await;

        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("server accepts");
        stream
            .write_all(
                b"POST / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 10\r\n\r\nabc",
            )
            .await
            .expect("the head and part of the body write");
        tokio::time::timeout(STALL_BOUND, reading.notified())
            .await
            .expect("the handler starts reading");
        tokio::time::sleep(LIMIT / 2).await;
        stream.write_all(b"defghij").await.expect("the rest writes");
        let response = read_until_closed(&mut stream).await;

        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
        assert!(response.ends_with("\r\n\r\n10"), "{response}");

        let _ = shutdown_sender.send(());
        server
            .await
            .expect("server task joins")
            .expect("server exits cleanly");
    }

    /// A client sending `Expect: 100-continue` sends no body until it is told
    /// to, which happens on the handler's first read, so waiting for the
    /// `100 Continue` is not a stall: the clock starts with the read that
    /// sends it, here two limits after the request arrived.
    #[cfg(feature = "http1")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_client_awaiting_100_continue_is_not_a_stall() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

        const LIMIT: Duration = Duration::from_millis(400);
        const CONTINUE: &[u8] = b"HTTP/1.1 100 Continue\r\n\r\n";

        let reading = std::sync::Arc::new(tokio::sync::Notify::new());
        let service = late_reading_service(LIMIT * 2, reading);
        let (address, shutdown_sender, server) = idle_timed_server_over(service, Some(LIMIT)).await;

        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("server accepts");
        stream
            .write_all(
                b"POST / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nExpect: 100-continue\r\nContent-Length: 3\r\n\r\n",
            )
            .await
            .expect("the head writes");
        let mut interim = vec![0_u8; CONTINUE.len()];
        tokio::time::timeout(STALL_BOUND, stream.read_exact(&mut interim))
            .await
            .expect("the server asks for the body")
            .expect("the interim response reads");
        assert_eq!(interim, CONTINUE, "{}", String::from_utf8_lossy(&interim));
        tokio::time::sleep(LIMIT / 2).await;
        stream.write_all(b"abc").await.expect("the body writes");
        let response = read_until_closed(&mut stream).await;

        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
        assert!(response.ends_with("\r\n\r\n3"), "{response}");

        let _ = shutdown_sender.send(());
        server
            .await
            .expect("server task joins")
            .expect("server exits cleanly");
    }

    /// A response the handler returned before the body stalled stands: its
    /// head may already be on the wire, so the stall ends that response's
    /// body rather than turning it into a 408.
    #[cfg(feature = "http1")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_response_returned_before_the_stall_stands() {
        use tokio::io::AsyncWriteExt as _;

        let document = kynos_openapi::Document::new(
            kynos_openapi::SpecVersion::V3_1,
            kynos_openapi::Info::new("Test", "1"),
        );
        // Answers at once, echoing the request body as its own.
        let echo = crate::router::service::Service::new(
            document,
            |request: crate::http::Request| async move {
                crate::http::Response::new(request.into_body())
            },
        );
        let (address, shutdown_sender, server) = idle_timed_server_over(echo, Some(IDLE)).await;

        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("server accepts");
        stream
            .write_all(STALLED_HEAD)
            .await
            .expect("request writes");
        let response = read_until_closed(&mut stream).await;

        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
        assert!(!response.contains(" 408 "), "{response}");

        let _ = shutdown_sender.send(());
        server
            .await
            .expect("server task joins")
            .expect("server exits cleanly");
    }

    /// The limit is on the gap, not the total: a body whose every gap is under
    /// it is read whole however long it takes altogether.
    #[cfg(feature = "http1")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_http1_body_arriving_steadily_outlives_its_idle_timeout() {
        use tokio::io::AsyncWriteExt as _;

        const LIMIT: Duration = Duration::from_secs(1);
        const GAP: Duration = Duration::from_millis(300);

        let (address, shutdown_sender, server) = idle_timed_server(Some(LIMIT)).await;

        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("server accepts");
        stream
            .write_all(
                b"POST / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 10\r\n\r\n",
            )
            .await
            .expect("head writes");
        // Five gaps of 300 ms: 1.5 s altogether, past the limit, and never 1 s
        // without a byte.
        for chunk in [&b"ab"[..], b"cd", b"ef", b"gh", b"ij"] {
            tokio::time::sleep(GAP).await;
            stream.write_all(chunk).await.expect("chunk writes");
        }
        let response = read_until_closed(&mut stream).await;

        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
        assert!(response.ends_with("\r\n\r\n10"), "{response}");

        let _ = shutdown_sender.send(());
        server
            .await
            .expect("server task joins")
            .expect("server exits cleanly");
    }

    /// `None` waits: a body that stalls well past [`IDLE`] is still read once
    /// the rest of it arrives.
    #[cfg(feature = "http1")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_disabled_idle_timeout_waits_for_the_rest_of_the_body() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

        let (address, shutdown_sender, server) = idle_timed_server(None).await;

        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("server accepts");
        stream
            .write_all(STALLED_HEAD)
            .await
            .expect("request writes");

        let mut early = [0_u8; 1];
        let answered = tokio::time::timeout(IDLE * 3, stream.read(&mut early)).await;
        assert!(
            answered.is_err(),
            "the server answered a body it was told to wait for"
        );

        stream.write_all(b"defghij").await.expect("the rest writes");
        let mut response = vec![0_u8; 256];
        let read = tokio::time::timeout(STALL_BOUND, stream.read(&mut response))
            .await
            .expect("the server answers within the bound")
            .expect("the response reads");
        let response = String::from_utf8_lossy(&response[..read]);
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");

        let _ = shutdown_sender.send(());
        server
            .await
            .expect("server task joins")
            .expect("server exits cleanly");
    }

    /// A request body that yields one frame and then nothing, ever.
    #[cfg(feature = "http2")]
    struct SendsOnceThenStalls(Option<bytes::Bytes>);

    #[cfg(feature = "http2")]
    impl http_body::Body for SendsOnceThenStalls {
        type Data = bytes::Bytes;
        type Error = std::convert::Infallible;

        fn poll_frame(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Result<http_body::Frame<bytes::Bytes>, Self::Error>>> {
            match self.get_mut().0.take() {
                Some(data) => std::task::Poll::Ready(Some(Ok(http_body::Frame::data(data)))),
                None => std::task::Poll::Pending,
            }
        }
    }

    /// Over HTTP/2 the stall held one stream, so the 408 ends that stream and
    /// carries no `Connection` field, which RFC 9113 §8.2.2 makes malformed.
    #[cfg(feature = "http2")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_http2_body_that_stalls_is_answered_408_on_its_stream() {
        use hyper_util::rt::{TokioExecutor, TokioIo};

        let (address, shutdown_sender, server) = idle_timed_server(Some(IDLE)).await;

        let stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("server accepts");
        let (mut sender, connection) =
            hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
                .await
                .expect("HTTP/2 handshake completes");
        let connection = tokio::spawn(connection);
        let request = hyper::Request::builder()
            .method("POST")
            .uri("http://localhost/")
            .body(SendsOnceThenStalls(Some(bytes::Bytes::from_static(b"abc"))))
            .expect("request builds");
        let response = tokio::time::timeout(STALL_BOUND, sender.send_request(request))
            .await
            .expect("the server answers within the bound")
            .expect("the request is answered");

        assert_eq!(response.status(), crate::http::StatusCode::REQUEST_TIMEOUT);
        assert!(
            !response
                .headers()
                .contains_key(crate::http::header::CONNECTION),
            "{:?}",
            response.headers()
        );
        assert_eq!(
            response.headers().get(crate::http::header::CONTENT_TYPE),
            Some(&crate::http::HeaderValue::from_static(
                "application/problem+json"
            )),
            "{:?}",
            response.headers()
        );
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .expect("the problem document reads")
            .to_bytes();
        let body = String::from_utf8_lossy(&body);
        assert!(
            body.contains(r#""detail":"the request body sent nothing for 200ms""#),
            "the 408 names the limit the body passed: {body}"
        );

        drop(sender);
        connection.abort();
        let _ = shutdown_sender.send(());
        server
            .await
            .expect("server task joins")
            .expect("server exits cleanly");
    }
}
