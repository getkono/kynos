# Production

What `Server::new` already bounds, and what an application must still do
itself. Paths are relative to `kynos`, and every figure is the default a
`Server` or config starts from; each has a setter of the same name.

This is a guide, not a contract: nothing here is normative. The API reference
states each bound precisely, and wins where the two differ.

## Nothing to do

Each of these holds with no configuration, so none of it needs copying from
another framework.

| Concern | Default | Setter |
| --- | --- | --- |
| Open connections | At most 10,000 across every listener, TLS handshakes included. At the cap the server stops accepting, and new connections wait in the kernel's listen queue until one closes. Each socket the server binds asks for as long a queue as the kernel allows, so its ceiling governs: `net.core.somaxconn` on Linux, 4096 by default since 5.4. Past it, Linux drops new connection attempts, and their clients retry after a second or more. | `server::Server::max_connections`, `listen_backlog` |
| A client slow to send its request head | 30 seconds from accept for a connection's first head, under either protocol, and for each later HTTP/1 head, so an idle kept-alive HTTP/1 connection closes 30 seconds after its last response. | `server::protocol::http1::Http1Config::header_read_timeout` |
| An idle HTTP/2 connection | Sent a GOAWAY and closed after 30 seconds with no stream in flight. A PING after 30 seconds of silence must be answered within 20. | `server::protocol::http2::Http2Config::idle_timeout`, `keep_alive` |
| A request body that stalls | Fails after 30 seconds without a frame, answered with a declared `408`. A steady upload is never cut short. | `server::Server::request_body_idle_timeout` |
| A request body too large to buffer | Refused past 2 MiB with a declared `413`, before a byte is read when `Content-Length` says so. | `middleware::limits::body_size::BodySize` |
| Header floods | HTTP/1: 100 headers and a 408 KiB buffer. HTTP/2: 200 concurrent streams, a 16 KiB header list, 1 MiB windows, and caps on reset streams. | `Http1Config`, `Http2Config` |
| A peer that vanished without closing | TCP keepalive probes after 60 seconds idle, every 15, and `TCP_USER_TIMEOUT` at 195 seconds on Linux, Android, Fuchsia and Cygwin. There its connection slot is released within 195 seconds either way, at Linux's default of nine probes. Elsewhere an idle connection's release depends on the OS's probe count, and an HTTP/1 peer lost mid-response is bounded only by the kernel's retransmission timeouts, since no probe is sent while data is unacknowledged. | `server::Server::tcp_keepalive` |
| Running out of file descriptors | On Unix, startup raises a soft `RLIMIT_NOFILE` short of the connection cap, plus one per listener and 128 to spare, as far as the hard limit, and warns naming both numbers where that is still short; raise the hard limit (`LimitNOFILE=`, `ulimit -Hn`) or lower the cap. Each `Server` checks only its own cap, so several in one process (an application and its metrics endpoint) can together exceed a limit each found sufficient, unwarned; size the limit for the sum of their caps. A failed accept (`EMFILE`, `ENFILE`, out of memory) waits from 10 ms, doubling to a one-second cap, and retries without limit, so a descriptor a closing connection frees is used within a second. A failure belonging to one queued connection retries at once. | none |
| A slow TLS handshake, under `tls` | Abandoned after 10 seconds, and counted under the connection cap while it runs. | `server::tls::TlsConfig::handshake_timeout` |
| TLS resumption, under `tls` | Stateless session tickets under random keys the process rotates once six hours pass, so a returning client skips the full handshake. Replicas that should resume one another's sessions share keys through `SessionResumption::SharedTickets`. | `server::tls::TlsConfig::session_resumption` |
| Forwarding headers | Trusted from nobody: every request resolves to its socket peer until the router names the proxies it trusts. | `Router::trusted_proxies` |

### The one thing that is not on by default

Shutdown. Without a trigger, nothing catches a signal, so `SIGTERM` ends the
process with every request in flight. Hand one over:

```rust
Server::new(service)
    .bind(address)
    .graceful_shutdown(Shutdown::signals())
    .serve()
    .await
```

`server::shutdown::Shutdown::signals` listens for `SIGINT` and `SIGTERM` on
Unix and the console events on Windows. On the first, the server stops
accepting and drains in-flight requests for up to 25 seconds, under the common
30-second orchestrator termination window; a second forces it.
`server::Server::shutdown_timeout` moves the deadline. Behind a load balancer,
`server::Server::deregistration_delay` keeps accepting for a while after the
first signal and only then drains, and a readiness probe answered from the
`server::lifecycle::Readiness` extractor reports not-ready for that while, so
the instance leaves rotation before any connection is refused; the delay and
the deadline add up. A long-lived response
ends itself on the `server::lifecycle::Draining` extractor, which
`response::stream::sse::Sse::end_on_shutdown` does for an event stream under `openapi32`.
[`graceful_shutdown`](../crates/kynos/examples/graceful_shutdown.rs) shows each
trigger.

## What to do, and why

Each entry here follows from a bound stated above, the deployment's own
topology, or a standard, so none rests on a measurement. Advice whose case is a
cost (worker threads under a CPU quota, blocking work, the allocator,
compression) belongs here only once a recorded
[kynos-bench](https://github.com/getkono/kynos-bench) result justifies it:
advice copied from another framework, or reasoned without a measurement, is
what this page exists to replace. kynos-bench has not yet recorded those
results, so none of that advice is here, and nothing below tunes a cost.

### Operating system limits

- Size the hard `RLIMIT_NOFILE` for the connection cap: `LimitNOFILE=` under
  systemd, `ulimit -Hn`, or the container runtime's `nofile` limit. Startup
  raises the soft limit only as far as the hard one allows, and past it
  accepts fail and are retried, so clients stall while the server runs.
  Several `Server`s in one process need the sum of their caps.
- Raise `net.core.somaxconn` only where a burst at the connection cap must
  queue more than its value, 4096 by default: it caps the queue
  `listen_backlog` asks for. It belongs to the network namespace, so a
  container sets it in its own (`docker run --sysctl`, a pod's
  `securityContext.sysctls`) rather than inheriting the host's. Kubernetes
  classes it as an unsafe sysctl, so the kubelet rejects such a pod unless its
  node lists the sysctl in `--allowed-unsafe-sysctls`.
- Leave `server::Server::accept_shards`, on Linux and Android, at one until
  your own measurement shows accepting is the bottleneck. Each listener has one accept task, which
  is enough unless connections are both many and short-lived. Shards are
  `SO_REUSEPORT` sockets, which any process of the same user may join, so run
  a sharded server as a user of its own.

### Behind a proxy or CDN

- Name the proxies, or every client address is the proxy's.
  `Router::trusted_proxies` takes `http::forwarded::TrustedProxies`: `hops`
  where the proxies' addresses change under you, as a managed load balancer's
  do, and `networks` or `addresses` where they are known. Name the
  `ProxyHeader` the proxy appends to, since the other passes through whatever
  the client sent. `everyone` is right only where nothing but the proxies can
  reach the service. A balancer that relays TCP rather than HTTP names the
  client only through `server::Server::proxy_protocol`.
- Set the proxy's upstream idle timeout below 30 seconds. Kynos closes an idle
  HTTP/1 connection 30 seconds after its last response, and an idle HTTP/2
  one 30 seconds after its last stream; a proxy that keeps one pooled longer
  can send a request down a connection Kynos is closing, and answers it with
  a `502`. nginx's upstream `keepalive_timeout` and an AWS Application Load
  Balancer's idle timeout both default to 60 seconds. Raising
  `header_read_timeout` and `idle_timeout` above the proxy's instead also
  lengthens what a slow client may hold, so do that only where the proxy alone
  reaches the service.
- Size the proxy's upstream pool, summed across every proxy instance, below
  `max_connections`: each pooled connection holds a permit and a descriptor
  while idle, and connections past the cap wait in the listen queue.
- Retry only what may be retried. A proxy must not automatically retry a
  request with a non-idempotent method (RFC 9110 §9.2.2), however its
  connection failed, so enable proxy retries for idempotent methods alone.

### At the edge

- TLS terminated by Kynos, under `tls`: resumption needs nothing on one
  instance. Where a balancer spreads a client's connections across replicas,
  or a restart should keep sessions, give every replica the same keys through
  `SessionResumption::SharedTickets`, and rotate them yourself: Kynos rotates
  none it was given. A mutual-TLS service that cannot let a ticket key vouch
  for a client's identity uses `SessionResumption::Cache` or `Disabled`
  instead; `server::tls::SessionResumption` says why.
- TLS terminated by the proxy, under `http2`: the proxy may speak HTTP/2 to
  Kynos over plaintext with prior knowledge, since a plaintext connection's
  protocol is read from its first bytes. Each such connection carries at most
  `max_concurrent_streams` requests at once, 200 by default, so a proxy
  multiplexing every client onto few connections sizes its pool by it, or the
  application raises it.

### Load balancers

- Answer the readiness probe from `server::lifecycle::Readiness`, and never
  the liveness probe: not ready is the right answer while the instance leaves
  rotation, and a failed liveness probe asks for a restart instead.
- Set `server::Server::deregistration_delay` to how long the balancer takes to
  stop routing once readiness fails: its probe interval times its failure
  threshold, plus its own propagation, such as Kubernetes removing the
  instance from a Service's endpoints.
- Fit that delay plus `shutdown_timeout` inside the orchestrator's
  termination window, 30 seconds by default under Kubernetes
  (`terminationGracePeriodSeconds`); widen the window rather than shortening
  the drain where in-flight requests need it.

kynos-bench runs on a Linux rig. Nothing it records speaks for another
operating system or for a shape it has not measured; its
[methodology](https://github.com/getkono/kynos-bench/blob/master/docs/METHODOLOGY.md)
says what it measures.
