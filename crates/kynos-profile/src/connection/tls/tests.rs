use std::io;
use std::net::{Ipv4Addr, Shutdown, TcpListener};

use super::{Tunnel, sealed};

/// A peer that ends the socket without a `close_notify` truncates the
/// session, so [`Tunnel::serve`] reports it rather than a clean close: the
/// peer here takes the client's first flight unread and closes its side.
#[test]
fn a_connection_ended_without_close_notify_is_served_as_an_error() {
    let sealed = sealed("conn-tls-full");
    let runner = sealed.scenario.driver.runner();
    let endpoints = sealed.endpoints(&runner);
    let peer = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a loopback port is free");
    let address = peer.local_addr().expect("a bound listener has an address");
    let mut tunnel = Tunnel::connect(&runner, address, endpoints.client());
    tunnel.client.send(&[]);
    let (accepted, _) = peer.accept().expect("the client connected");
    // Only the write half, so the unread first flight resets nothing.
    accepted
        .shutdown(Shutdown::Write)
        .expect("the peer ends its side");
    let ended = tunnel
        .serve(&runner)
        .expect_err("a socket's end without close_notify");
    assert_eq!(ended.kind(), io::ErrorKind::UnexpectedEof);
    assert_eq!(tunnel.handshake(), None);
}
