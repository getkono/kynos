//! Client resolution over `Forwarded` and `X-Forwarded-For`/`-Proto`.
//!
//! Each line of the input is one field line, written to all three fields, and
//! resolved from three peers under every trust policy below. Trusting nobody,
//! or trusting no hop while the peer is no proxy, answers with the peer and no
//! scheme whatever the fields say. A scheme is lowercased, and whether the
//! client was secure is exactly whether that scheme is `https`.
//!
//! Trusting `n` hops never reaches past the `n`th element from the right, held
//! two ways. Three known hops written after the input's lines are the three
//! nearest the service, so trusting one, two or three of them answers with
//! exactly that hop's address, whatever the input holds. And under
//! `X-Forwarded-For`, whose elements are its comma-separated members, the
//! client is the one the rightmost `n` members alone resolve to.

#![no_main]

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use kynos::http::{
    HeaderMap, HeaderName, HeaderValue,
    forwarded::{Forwarded, ProxyHeader, TrustedProxies},
};
use libfuzzer_sys::fuzz_target;

const FIELDS: [&str; 3] = ["forwarded", "x-forwarded-for", "x-forwarded-proto"];

/// The three nearest hops, nearest last, as each field writes them.
const NEAREST: [(ProxyHeader, &str, &str); 2] = [
    (
        ProxyHeader::Forwarded,
        "forwarded",
        "for=192.0.2.1, for=\"[2001:db8::2]:4711\", for=192.0.2.3",
    ),
    (
        ProxyHeader::XForwarded,
        "x-forwarded-for",
        "192.0.2.1, 2001:db8::2, 192.0.2.3",
    ),
];

/// The address of each of [`NEAREST`]'s hops, nearest last.
const NEAREST_ADDRESSES: [IpAddr; 3] = [
    IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
    IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 2)),
    IpAddr::V4(Ipv4Addr::new(192, 0, 2, 3)),
];

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
                TrustedProxies::everyone(header),
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

        for (header, field, hops) in NEAREST {
            let mut extended = headers.clone();
            extended.append(
                HeaderName::from_static(field),
                HeaderValue::from_static(hops),
            );
            for (count, expected) in (1..=3).zip(NEAREST_ADDRESSES.iter().rev()) {
                let trusted = TrustedProxies::hops(header, count);
                let resolved = Forwarded::resolve(&extended, peer, &trusted);
                assert_eq!(
                    resolved.client(),
                    Some(*expected),
                    "{count} hops of {header:?}"
                );
            }
        }

        let members = members(&headers);
        for count in [1, 2, 3] {
            let mut nearest = HeaderMap::new();
            let kept = &members[members.len().saturating_sub(count)..];
            if !kept.is_empty() {
                let line =
                    HeaderValue::from_bytes(&kept.join(&b","[..])).expect("members of a field");
                nearest.append(HeaderName::from_static("x-forwarded-for"), line);
            }
            let trusted = TrustedProxies::hops(ProxyHeader::XForwarded, count);
            assert_eq!(
                Forwarded::resolve(&headers, peer, &trusted).client(),
                Forwarded::resolve(&nearest, peer, &trusted).client(),
                "{count} hops of {kept:?}",
            );
        }
    }
});

/// Every non-blank `X-Forwarded-For` member, left to right, without the
/// optional whitespace around it.
fn members(headers: &HeaderMap) -> Vec<&[u8]> {
    headers
        .get_all("x-forwarded-for")
        .iter()
        .flat_map(|line| line.as_bytes().split(|&byte| byte == b','))
        .map(<[u8]>::trim_ascii)
        .filter(|member| !member.is_empty())
        .collect()
}
