//! Client resolution over `Forwarded` and `X-Forwarded-For`/`-Proto`.
//!
//! Each line of the input is one field line, written to all three fields, and
//! resolved from three peers under every trust policy below. Trusting nobody,
//! or trusting no hop while the peer is no proxy, answers with the peer and no
//! scheme whatever the fields say. A scheme is lowercased, and whether the
//! client was secure is exactly whether that scheme is `https`.

#![no_main]

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use kynos::http::{
    HeaderMap, HeaderName, HeaderValue,
    forwarded::{Forwarded, ProxyHeader, TrustedProxies},
};
use libfuzzer_sys::fuzz_target;

const FIELDS: [&str; 3] = ["forwarded", "x-forwarded-for", "x-forwarded-proto"];

fuzz_target!(|data: &[u8]| {
    let mut headers = HeaderMap::new();
    for line in data.split(|&byte| byte == b'\n') {
        let Ok(value) = HeaderValue::from_bytes(line) else {
            return;
        };
        for field in FIELDS {
            headers.append(HeaderName::from_static(field), value.clone());
        }
    }

    let peers = [
        None,
        Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 443)),
        Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 443)),
    ];

    for peer in peers {
        let resolved = Forwarded::resolve(&headers, peer, &TrustedProxies::none());
        assert_eq!(resolved.client(), peer.map(|peer| peer.ip()));
        assert_eq!(resolved.proto(), None);

        for header in [ProxyHeader::Forwarded, ProxyHeader::XForwarded] {
            let untrusted = Forwarded::resolve(&headers, peer, &TrustedProxies::hops(header, 0));
            assert_eq!(untrusted.client(), peer.map(|peer| peer.ip()));
            assert_eq!(untrusted.proto(), None);

            let policies = [
                TrustedProxies::hops(header, 1),
                TrustedProxies::hops(header, 3),
                TrustedProxies::hops(header, usize::MAX),
                TrustedProxies::networks(
                    header,
                    [
                        (IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
                        (IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
                    ],
                ),
            ];
            for trusted in &policies {
                let resolved = Forwarded::resolve(&headers, peer, trusted);
                if let Some(proto) = resolved.proto() {
                    assert_eq!(proto, proto.to_ascii_lowercase());
                }
                assert_eq!(
                    resolved.client_is_secure(),
                    resolved.proto().map(|proto| proto == "https")
                );
            }
        }
    }
});
