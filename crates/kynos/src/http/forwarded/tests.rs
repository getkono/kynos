use std::net::{IpAddr, SocketAddr};

use super::{Forwarded, TrustedProxies, node_address, within};
use crate::http::{HeaderMap, HeaderValue};

/// A header map from pairs, appending so a repeated name stays repeated.
fn map(fields: &[(&str, &str)]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in fields {
        headers.append(
            crate::http::HeaderName::from_bytes(name.as_bytes()).expect("a legal field name"),
            HeaderValue::from_str(value).expect("a printable field"),
        );
    }
    headers
}

fn ip(text: &str) -> IpAddr {
    text.parse().expect("an address")
}

fn peer(text: &str) -> SocketAddr {
    SocketAddr::new(ip(text), 4000)
}

/// Nothing is believed until the application says whom to believe.
///
/// The default is the whole security position: RFC 7239 section 8.1 says the
/// field "cannot be relied upon to be correct, as it may be modified... by
/// every node on the way to the server, including the client making the
/// request."
#[test]
fn an_unconfigured_policy_believes_no_forwarding_field() {
    let headers = map(&[
        ("forwarded", "for=203.0.113.7;proto=https"),
        ("x-forwarded-for", "203.0.113.8"),
    ]);

    let resolved = Forwarded::resolve(&headers, Some(peer("10.0.0.1")), &TrustedProxies::none());

    assert_eq!(resolved.client(), Some(ip("10.0.0.1")));
    assert_eq!(resolved.proto(), None);
    assert_eq!(resolved.client_is_secure(), None);
}

/// One trusted hop resolves one element, and no more.
#[test]
fn one_trusted_hop_reads_one_element() {
    // Two elements: the client, then a proxy the client could have invented.
    let headers = map(&[("forwarded", "for=198.51.100.9, for=203.0.113.7")]);

    let resolved = Forwarded::resolve(&headers, Some(peer("10.0.0.1")), &TrustedProxies::hops(1));

    assert_eq!(
        resolved.client(),
        Some(ip("203.0.113.7")),
        "one hop of trust must not reach past the element the trusted proxy wrote"
    );
}

/// Trusting two hops reaches the second element.
#[test]
fn two_trusted_hops_reach_the_second_element() {
    let headers = map(&[("forwarded", "for=198.51.100.9, for=203.0.113.7")]);

    let resolved = Forwarded::resolve(&headers, Some(peer("10.0.0.1")), &TrustedProxies::hops(2));

    assert_eq!(resolved.client(), Some(ip("198.51.100.9")));
}

/// A spoofed chain cannot reach further than the trust allows.
///
/// The attack this exists to stop: a client writes a long `Forwarded` itself,
/// hoping the service reads the leftmost element and buckets a rate limit
/// against an address of the client's choosing.
#[test]
fn a_forged_chain_cannot_outrun_the_configured_trust() {
    let headers = map(&[(
        "forwarded",
        "for=1.1.1.1, for=2.2.2.2, for=3.3.3.3, for=203.0.113.7",
    )]);

    let resolved = Forwarded::resolve(&headers, Some(peer("10.0.0.1")), &TrustedProxies::hops(1));

    assert_eq!(resolved.client(), Some(ip("203.0.113.7")));
}

/// A trusted network believes an element written by a sender inside it.
#[test]
fn a_trusted_network_reads_the_element_its_member_wrote() {
    let headers = map(&[("forwarded", "for=203.0.113.7")]);
    let trusted = TrustedProxies::networks([(ip("10.0.0.0"), 8)]);

    let resolved = Forwarded::resolve(&headers, Some(peer("10.4.5.6")), &trusted);

    assert_eq!(resolved.client(), Some(ip("203.0.113.7")));
}

/// A peer outside every trusted network is not believed.
#[test]
fn a_sender_outside_the_trusted_networks_is_not_believed() {
    let headers = map(&[("forwarded", "for=203.0.113.7")]);
    let trusted = TrustedProxies::networks([(ip("10.0.0.0"), 8)]);

    let resolved = Forwarded::resolve(&headers, Some(peer("192.0.2.5")), &trusted);

    assert_eq!(resolved.client(), Some(ip("192.0.2.5")));
}

/// `X-Forwarded-For` is read only where `Forwarded` is absent.
///
/// Reading both risks pairing one hop's address with another hop's scheme.
#[test]
fn the_specified_field_wins_over_the_de_facto_one() {
    let headers = map(&[
        ("forwarded", "for=203.0.113.7;proto=https"),
        ("x-forwarded-for", "198.51.100.9"),
        ("x-forwarded-proto", "http"),
    ]);

    let resolved = Forwarded::resolve(&headers, Some(peer("10.0.0.1")), &TrustedProxies::hops(1));

    assert_eq!(resolved.client(), Some(ip("203.0.113.7")));
    assert_eq!(resolved.proto(), Some("https"));
}

/// With no `Forwarded`, the de-facto pair is read.
#[test]
fn the_de_facto_pair_is_read_when_it_is_all_there_is() {
    let headers = map(&[
        ("x-forwarded-for", "198.51.100.9, 203.0.113.7"),
        ("x-forwarded-proto", "https"),
    ]);

    let resolved = Forwarded::resolve(&headers, Some(peer("10.0.0.1")), &TrustedProxies::hops(1));

    assert_eq!(resolved.client(), Some(ip("203.0.113.7")));
    assert_eq!(resolved.client_is_secure(), Some(true));
}

/// Every `nodename` form section 6 defines, and what each yields.
///
/// The table is the grammar. `unknown` and an `obfnode` are identifiers rather
/// than addresses, so they resolve to nothing rather than to a guess.
#[test]
fn every_node_identifier_form_is_read_the_way_the_grammar_defines_it() {
    let cases: &[(&str, Option<&str>)] = &[
        ("192.0.2.60", Some("192.0.2.60")),
        ("192.0.2.60:4711", Some("192.0.2.60")),
        ("[2001:db8:cafe::17]", Some("2001:db8:cafe::17")),
        ("[2001:db8:cafe::17]:4711", Some("2001:db8:cafe::17")),
        // Outside the grammar, but proxies send it on `X-Forwarded-For`.
        ("2001:db8:cafe::17", Some("2001:db8:cafe::17")),
        ("unknown", None),
        ("unknown:4711", None),
        ("_gazonk", None),
        ("_hidden:_port", None),
        ("", None),
    ];

    for (node, expected) in cases {
        assert_eq!(
            node_address(node),
            expected.map(ip),
            "`{node}` was not read the way section 6 defines it"
        );
    }
}

/// Prefix matching, over the boundaries a hand-rolled one gets wrong.
#[test]
fn a_network_contains_exactly_the_addresses_its_prefix_names() {
    let cases: &[(&str, &str, u8, bool)] = &[
        ("10.4.5.6", "10.0.0.0", 8, true),
        ("11.4.5.6", "10.0.0.0", 8, false),
        // A partial octet, which is where an off-by-one lands.
        ("10.127.0.1", "10.0.0.0", 9, true),
        ("10.128.0.1", "10.0.0.0", 9, false),
        // A whole-octet boundary.
        ("192.168.1.1", "192.168.0.0", 16, true),
        ("192.169.1.1", "192.168.0.0", 16, false),
        // /0 matches everything of the same family.
        ("203.0.113.1", "0.0.0.0", 0, true),
        // /32 is one address.
        ("203.0.113.1", "203.0.113.1", 32, true),
        ("203.0.113.2", "203.0.113.1", 32, false),
        // A prefix longer than the address has bits for matches nothing.
        ("203.0.113.1", "203.0.113.1", 33, false),
        ("2001:db8::1", "2001:db8::", 32, true),
        ("2001:db9::1", "2001:db8::", 32, false),
    ];

    for (address, network, prefix, expected) in cases {
        assert_eq!(
            within(ip(address), ip(network), *prefix),
            *expected,
            "{address} in {network}/{prefix}"
        );
    }
}

/// The two families never match across each other.
///
/// Mapping one onto the other would make `::ffff:10.0.0.1` match a `10.0.0.0/8`
/// rule its author never wrote.
#[test]
fn a_network_never_matches_across_address_families() {
    assert!(!within(ip("::ffff:10.0.0.1"), ip("10.0.0.0"), 8));
    assert!(!within(ip("10.0.0.1"), ip("::"), 0));
}

/// A request that arrived on no socket, with nothing trusted to name one.
#[test]
fn a_request_from_no_socket_resolves_to_no_client() {
    let resolved = Forwarded::resolve(&HeaderMap::new(), None, &TrustedProxies::none());
    assert_eq!(resolved.client(), None);
}
