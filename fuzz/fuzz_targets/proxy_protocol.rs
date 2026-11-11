//! The PROXY protocol header reader a listener runs before a trusted proxy's
//! connection is served.
//!
//! The input is the start of a connection, read as the bytes arrive: every
//! prefix of it reads as a header still arriving until one prefix decides,
//! and every longer prefix decides the same way, so how the socket splits
//! its reads changes nothing. A whole header is decided exactly at its last
//! byte, never longer than version 1's 107 bytes nor shorter than version 2's
//! fixed 16.
//!
//! Whatever the input, a header naming a client drawn from its first bytes,
//! written in either version, reads back as that client and that header's
//! length when the input follows it.

#![no_main]

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use kynos::__private::fuzz::{ProxyRead, proxy_header};
use libfuzzer_sys::fuzz_target;

const V2_SIGNATURE: &[u8] = b"\r\n\r\n\0\r\nQUIT\n";

fuzz_target!(|data: &[u8]| {
    let whole = proxy_header(data);
    let mut decided = None;
    for end in 0..=data.len() {
        let read = proxy_header(&data[..end]);
        match decided {
            None if read == ProxyRead::Partial => {}
            None => {
                if let ProxyRead::Complete(_, length) = read {
                    assert_eq!(length, end, "decided before or after its last byte");
                    assert!(
                        (data[0] == b'P' && length <= 107) || (data[0] == b'\r' && length >= 16),
                        "{length} bytes"
                    );
                }
                decided = Some(read);
            }
            Some(decided) => assert_eq!(read, decided, "{end} bytes"),
        }
    }
    assert_eq!(whole, decided.unwrap_or(ProxyRead::Partial));

    if let Some(client) = client(data) {
        for header in [v1(client), v2(client)] {
            let mut connection = header.clone();
            connection.extend_from_slice(data);
            assert_eq!(
                proxy_header(&connection),
                ProxyRead::Complete(Some(client), header.len()),
                "{header:?}"
            );
        }
    }
});

/// A client drawn from the input's first bytes: IPv4 or IPv6 by the first
/// byte, then the address and the port.
fn client(data: &[u8]) -> Option<SocketAddr> {
    let (&family, rest) = data.split_first()?;
    let address: IpAddr = if family % 2 == 0 {
        let octets: [u8; 4] = rest.get(..4)?.try_into().ok()?;
        Ipv4Addr::from(octets).into()
    } else {
        let octets: [u8; 16] = rest.get(..16)?.try_into().ok()?;
        Ipv6Addr::from(octets).into()
    };
    let at = if address.is_ipv4() { 4 } else { 16 };
    let port = u16::from_be_bytes(rest.get(at..at + 2)?.try_into().ok()?);
    Some(SocketAddr::new(address, port))
}

/// `client` as a version 1 header writes it.
fn v1(client: SocketAddr) -> Vec<u8> {
    let (protocol, destination) = match client {
        SocketAddr::V4(_) => ("TCP4", "192.0.2.2"),
        SocketAddr::V6(_) => ("TCP6", "2001:db8::2"),
    };
    format!(
        "PROXY {protocol} {} {destination} {} 443\r\n",
        client.ip(),
        client.port()
    )
    .into_bytes()
}

/// `client` as a version 2 `PROXY` header over TCP writes it.
fn v2(client: SocketAddr) -> Vec<u8> {
    let mut header = V2_SIGNATURE.to_vec();
    header.push(0x21);
    let mut block = match client {
        SocketAddr::V4(client) => {
            header.push(0x11);
            let mut block = client.ip().octets().to_vec();
            block.extend_from_slice(&[192, 0, 2, 2]);
            block
        }
        SocketAddr::V6(client) => {
            header.push(0x21);
            let mut block = client.ip().octets().to_vec();
            block.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
            block
        }
    };
    block.extend_from_slice(&client.port().to_be_bytes());
    block.extend_from_slice(&443_u16.to_be_bytes());
    let length = u16::try_from(block.len()).expect("an address block fits the length field");
    header.extend_from_slice(&length.to_be_bytes());
    header.extend_from_slice(&block);
    header
}
