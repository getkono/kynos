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
| Open connections | At most 10,000 across every listener, TLS handshakes included. At the cap the server stops accepting, and new connections wait in the kernel's listen queue until one closes. | `server::Server::max_connections` |
| A client slow to send its request head | 30 seconds from accept for a connection's first head, under either protocol, and for each later HTTP/1 head, so an idle kept-alive HTTP/1 connection closes 30 seconds after its last response. | `server::protocol::http1::Http1Config::header_read_timeout` |
| An idle HTTP/2 connection | Sent a GOAWAY and closed after 30 seconds with no stream in flight. A PING after 30 seconds of silence must be answered within 20. | `server::protocol::http2::Http2Config::idle_timeout`, `keep_alive` |
| A request body that stalls | Fails after 30 seconds without a frame, answered with a declared `408`. A steady upload is never cut short. | `server::Server::request_body_idle_timeout` |
| A request body too large to buffer | Refused past 2 MiB with a declared `413`, before a byte is read when `Content-Length` says so. | `middleware::limits::body_size::BodySize` |
| Header floods | HTTP/1: 100 headers and a 408 KiB buffer. HTTP/2: 200 concurrent streams, a 16 KiB header list, 1 MiB windows, and caps on reset streams. | `Http1Config`, `Http2Config` |
| A peer that vanished without closing | TCP keepalive probes after 60 seconds idle, every 15, and `TCP_USER_TIMEOUT` at 195 seconds on Linux, Android, Fuchsia and Cygwin. There its connection slot is released within 195 seconds either way, at Linux's default of nine probes. Elsewhere an idle connection's release depends on the OS's probe count, and an HTTP/1 peer lost mid-response is bounded only by the kernel's retransmission timeouts, since no probe is sent while data is unacknowledged. | `server::Server::tcp_keepalive` |
| Running out of file descriptors | A failed accept (`EMFILE`, `ENFILE`, out of memory) waits from 10 ms, doubling to a one-second cap, and retries without limit, so a descriptor a closing connection frees is used within a second. A failure belonging to one queued connection retries at once. | none |
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
`server::Server::shutdown_timeout` moves the deadline. A long-lived response
ends itself on the `server::lifecycle::Draining` extractor, which
`response::stream::sse::Sse::end_on_shutdown` does for an event stream under `openapi32`.
[`graceful_shutdown`](../crates/kynos/examples/graceful_shutdown.rs) shows each
trigger.

## What to do, and why

An entry belongs here only once a recorded
[kynos-bench](https://github.com/getkono/kynos-bench) result justifies it:
advice copied from another framework, or reasoned without a measurement, is
what this page exists to replace. kynos-bench has not yet recorded the results
these depend on, so there is no entry yet, and the application needs no tuning
beyond the shutdown trigger above.

kynos-bench runs on a Linux rig. Nothing it records speaks for another
operating system or for a shape it has not measured; its
[methodology](https://github.com/getkono/kynos-bench/blob/master/docs/METHODOLOGY.md)
says what it measures.
