use std::net::{IpAddr, SocketAddr};

use super::{Forwarded, ProxyHeader, TrustedProxies, node_address, within};
use crate::http::{HeaderMap, HeaderValue, Request, body::Body};

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

/// A policy naming no field reads none, whatever addresses it was widened by.
///
/// `none().and_addresses(..)` says whom to believe but not where they wrote it,
/// and either guess reads a field the client may have written.
#[test]
fn a_policy_naming_no_field_believes_no_forwarding_field() {
    let headers = map(&[
        ("forwarded", "for=203.0.113.7;proto=https"),
        ("x-forwarded-for", "203.0.113.8"),
        ("x-forwarded-proto", "https"),
    ]);
    let trusted = TrustedProxies::none()
        .and_addresses([ip("10.0.0.1")])
        .and_networks([(ip("10.0.0.0"), 8)]);

    let resolved = Forwarded::resolve(&headers, Some(peer("10.0.0.1")), &trusted);

    assert!(trusted.trusts_nobody());
    assert_eq!(resolved.client(), Some(ip("10.0.0.1")));
    assert_eq!(resolved.client_is_secure(), None);
}

/// One trusted hop resolves one element, and no more.
#[test]
fn one_trusted_hop_reads_one_element() {
    // Two elements: the client, then a proxy the client could have invented.
    let headers = map(&[("forwarded", "for=198.51.100.9, for=203.0.113.7")]);

    let resolved = Forwarded::resolve(
        &headers,
        Some(peer("10.0.0.1")),
        &TrustedProxies::hops(ProxyHeader::Forwarded, 1),
    );

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

    let resolved = Forwarded::resolve(
        &headers,
        Some(peer("10.0.0.1")),
        &TrustedProxies::hops(ProxyHeader::Forwarded, 2),
    );

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

    let resolved = Forwarded::resolve(
        &headers,
        Some(peer("10.0.0.1")),
        &TrustedProxies::hops(ProxyHeader::Forwarded, 1),
    );

    assert_eq!(resolved.client(), Some(ip("203.0.113.7")));
}

/// A trusted network believes an element written by a sender inside it.
#[test]
fn a_trusted_network_reads_the_element_its_member_wrote() {
    let headers = map(&[("forwarded", "for=203.0.113.7")]);
    let trusted = TrustedProxies::networks(ProxyHeader::Forwarded, [(ip("10.0.0.0"), 8)]);

    let resolved = Forwarded::resolve(&headers, Some(peer("10.4.5.6")), &trusted);

    assert_eq!(resolved.client(), Some(ip("203.0.113.7")));
}

/// A peer outside every trusted network is not believed.
#[test]
fn a_sender_outside_the_trusted_networks_is_not_believed() {
    let headers = map(&[("forwarded", "for=203.0.113.7")]);
    let trusted = TrustedProxies::networks(ProxyHeader::Forwarded, [(ip("10.0.0.0"), 8)]);

    let resolved = Forwarded::resolve(&headers, Some(peer("192.0.2.5")), &trusted);

    assert_eq!(resolved.client(), Some(ip("192.0.2.5")));
}

/// Behind a proxy that writes `Forwarded`, the client's own `X-Forwarded-*`
/// names nobody, and neither does it where the proxy wrote no `Forwarded`.
///
/// The twin of the case below. Falling back to the other field where the named
/// one is absent would hand the client the address whenever its proxy stated
/// none.
#[test]
fn a_client_written_x_forwarded_pair_is_not_read_behind_a_forwarded_proxy() {
    let trusted = TrustedProxies::hops(ProxyHeader::Forwarded, 1);
    let spoofed = [
        ("x-forwarded-for", "1.2.3.4"),
        ("x-forwarded-proto", "https"),
    ];

    let mut beside = spoofed.to_vec();
    beside.push(("forwarded", "for=203.0.113.7;proto=http"));
    let resolved = Forwarded::resolve(&map(&beside), Some(peer("10.0.0.1")), &trusted);
    assert_eq!(resolved.client(), Some(ip("203.0.113.7")));
    assert_eq!(resolved.client_is_secure(), Some(false));

    let resolved = Forwarded::resolve(&map(&spoofed), Some(peer("10.0.0.1")), &trusted);
    assert_eq!(resolved.client(), Some(ip("10.0.0.1")));
    assert_eq!(resolved.client_is_secure(), None);
}

/// Behind a proxy that appends to `X-Forwarded-For`, a `Forwarded` the client
/// wrote itself names nobody.
///
/// An AWS ALB, or a typical nginx, passes a client's `Forwarded` through
/// untouched. Reading it there lets the client pick its own address and scheme
/// on every request, which is the bucket a `ByClientAddress` limit counts.
#[test]
fn a_client_written_forwarded_is_not_read_behind_an_x_forwarded_for_proxy() {
    let headers = map(&[
        ("forwarded", "for=1.2.3.4;proto=https"),
        ("x-forwarded-for", "198.51.100.9"),
    ]);
    let trusted = TrustedProxies::hops(ProxyHeader::XForwarded, 1);

    let resolved = Forwarded::resolve(&headers, Some(peer("10.0.0.1")), &trusted);

    assert_eq!(resolved.client(), Some(ip("198.51.100.9")));
    assert_eq!(resolved.client_is_secure(), None);
}

/// Behind a proxy that writes the de-facto pair, the pair is read.
#[test]
fn the_de_facto_pair_is_read_behind_a_proxy_that_writes_it() {
    let headers = map(&[
        ("x-forwarded-for", "198.51.100.9, 203.0.113.7"),
        ("x-forwarded-proto", "https"),
    ]);
    let trusted = TrustedProxies::hops(ProxyHeader::XForwarded, 1);

    let resolved = Forwarded::resolve(&headers, Some(peer("10.0.0.1")), &trusted);

    assert_eq!(resolved.client(), Some(ip("203.0.113.7")));
    assert_eq!(resolved.client_is_secure(), Some(true));
}

/// An element that names no address still occupies the position its proxy
/// wrote it at.
///
/// The trusted proxy wrote the rightmost element; the one left of it is the
/// client's own word. Dropping an element that says `unknown` would slide that
/// client-written address into the trusted position, so the walk lands on the
/// `unknown` instead and resolves no address rather than a spoofable one. RFC
/// 7239 section 6 makes `unknown` and an `obfnode` identifiers, not addresses;
/// an element with no `for=` at all is still one proxy's hop.
#[test]
fn an_unknown_element_at_a_trusted_position_is_still_a_hop() {
    for element in [
        "for=unknown",
        "for=\"unknown:4711\"",
        "for=_hidden",
        "for=\"_hidden:_port\"",
        "for=not-an-address",
        "by=10.0.0.2;proto=https",
    ] {
        let headers = map(&[("forwarded", format!("for=203.0.113.9, {element}").as_str())]);

        let resolved = Forwarded::resolve(
            &headers,
            Some(peer("10.0.0.1")),
            &TrustedProxies::hops(ProxyHeader::Forwarded, 1),
        );

        assert_eq!(
            resolved.client(),
            None,
            "`{element}` was skipped, handing its trusted position to the client's own element"
        );
    }
}

/// The `X-Forwarded-For` twin of the case above.
#[test]
fn an_unknown_x_forwarded_for_entry_at_a_trusted_position_is_still_a_hop() {
    for entry in ["unknown", "_hidden", "not-an-address"] {
        let headers = map(&[("x-forwarded-for", format!("203.0.113.9, {entry}").as_str())]);
        let trusted = TrustedProxies::hops(ProxyHeader::XForwarded, 1);

        let resolved = Forwarded::resolve(&headers, Some(peer("10.0.0.1")), &trusted);

        assert_eq!(
            resolved.client(),
            None,
            "`{entry}` was skipped, handing its trusted position to the client's own entry"
        );
    }
}

/// An element with no address spends one hop, so trust reaching past it stops
/// exactly one element further left.
#[test]
fn an_unknown_element_spends_one_hop_of_trust() {
    let forwarded = map(&[(
        "forwarded",
        "for=198.51.100.1, for=203.0.113.9, for=unknown",
    )]);
    let x_forwarded_for = map(&[("x-forwarded-for", "198.51.100.1, 203.0.113.9, unknown")]);

    for (header, headers) in [
        (ProxyHeader::Forwarded, forwarded),
        (ProxyHeader::XForwarded, x_forwarded_for),
    ] {
        let trusted = TrustedProxies::hops(header, 2);
        let resolved = Forwarded::resolve(&headers, Some(peer("10.0.0.1")), &trusted);

        assert_eq!(resolved.client(), Some(ip("203.0.113.9")));
    }
}

/// An empty list element is no element at all.
///
/// RFC 9110 section 5.6.1.2 has a recipient ignore it, so a trailing comma
/// spends no hop.
#[test]
fn an_empty_list_element_is_not_a_hop() {
    let forwarded = map(&[("forwarded", "for=203.0.113.9, ")]);
    let x_forwarded_for = map(&[("x-forwarded-for", "203.0.113.9, ")]);

    for (header, headers) in [
        (ProxyHeader::Forwarded, forwarded),
        (ProxyHeader::XForwarded, x_forwarded_for),
    ] {
        let trusted = TrustedProxies::hops(header, 1);
        let resolved = Forwarded::resolve(&headers, Some(peer("10.0.0.1")), &trusted);

        assert_eq!(resolved.client(), Some(ip("203.0.113.9")));
    }
}

/// A delimiter inside a `quoted-string` value splits nothing.
///
/// RFC 7239 section 4 lets a `value` be a `quoted-string`, which may hold `,`
/// and `;`, and a `quoted-pair` may escape a `"` inside it. Splitting there
/// would invent a hop with no address, or a `for=` the proxy never wrote.
#[test]
fn a_delimiter_inside_a_quoted_value_splits_nothing() {
    for element in [
        r#"for=203.0.113.9;ext="a,b""#,
        r#"for=203.0.113.9;ext="a;for=198.51.100.66""#,
        r#"for=203.0.113.9;ext="a\",b""#,
    ] {
        let headers = map(&[("forwarded", element)]);

        let resolved = Forwarded::resolve(
            &headers,
            Some(peer("10.0.0.1")),
            &TrustedProxies::hops(ProxyHeader::Forwarded, 1),
        );

        assert_eq!(
            resolved.client(),
            Some(ip("203.0.113.9")),
            "`{element}` was split inside its quoted value"
        );
    }
}

/// A quote the client leaves open cannot absorb the elements trusted hops
/// appended after it, so the client cannot choose the element trust lands on.
#[test]
fn a_quote_the_client_leaves_open_swallows_no_trusted_hop() {
    for (field, client) in [
        (r#"for=198.51.100.66;ext=", for=203.0.113.9"#, "203.0.113.9"),
        (
            r#"for=198.51.100.66;ext=", for="[2001:db8::9]""#,
            "2001:db8::9",
        ),
        (
            r#"ext="a\", for=198.51.100.66, for=203.0.113.9"#,
            "203.0.113.9",
        ),
    ] {
        let headers = map(&[("forwarded", field)]);

        let resolved = Forwarded::resolve(
            &headers,
            Some(peer("10.0.0.1")),
            &TrustedProxies::hops(ProxyHeader::Forwarded, 1),
        );

        assert_eq!(resolved.client(), Some(ip(client)), "`{field}`");
    }
}

/// Repeated `Forwarded` lines are one list, in the order they were written.
///
/// RFC 9110 section 5.3 makes a repeated list field one comma-joined value, so
/// the second line's elements follow the first's, and each spends its own hop.
#[test]
fn repeated_forwarded_lines_are_one_chain_in_written_order() {
    let headers = map(&[
        ("forwarded", "for=198.51.100.1, for=203.0.113.9"),
        ("forwarded", "for=192.0.2.7, for=unknown"),
    ]);

    for (hops, client) in [
        (1, None),
        (2, Some("192.0.2.7")),
        (3, Some("203.0.113.9")),
        (4, Some("198.51.100.1")),
    ] {
        let resolved = Forwarded::resolve(
            &headers,
            Some(peer("10.0.0.1")),
            &TrustedProxies::hops(ProxyHeader::Forwarded, hops),
        );

        assert_eq!(resolved.client(), client.map(ip), "hops({hops})");
    }
}

/// The scheme is the one stated in the element the walk stops at, lowercased.
///
/// That element's `for=` is the client and its `proto=` is the scheme the
/// client connected with, both written by the same trusted hop (RFC 7239
/// section 5.4). A `proto=` nearer the service describes a connection between
/// proxies; one further out was written by a sender nobody trusts. Neither
/// stands in for a stop element that states none, across elements and across
/// repeated lines alike.
#[test]
fn the_scheme_is_read_from_the_element_the_walk_stops_at() {
    let headers = map(&[
        (
            "forwarded",
            "for=198.51.100.1;proto=ws, for=203.0.113.9;proto=http",
        ),
        (
            "forwarded",
            "for=192.0.2.7;proto=HTTPS, for=10.0.0.2;proto=http",
        ),
        ("forwarded", "for=10.0.0.3"),
    ]);

    for (hops, proto) in [
        (1, None),
        (2, Some("http")),
        (3, Some("https")),
        (4, Some("http")),
        (5, Some("ws")),
        (6, Some("ws")),
    ] {
        let resolved = Forwarded::resolve(
            &headers,
            Some(peer("10.0.0.1")),
            &TrustedProxies::hops(ProxyHeader::Forwarded, hops),
        );

        assert_eq!(resolved.proto(), proto, "hops({hops})");
    }
}

/// A scheme the client wrote is not read through a trusted hop that states
/// none.
///
/// The client sends `Forwarded: proto=https`, and the trusted proxy appends its
/// own element without a `proto=`. The walk stops at the proxy's element, so
/// the client's claim is never reached -- believing it would report https over
/// a plain-HTTP hop.
#[test]
fn a_client_written_scheme_is_not_read_through_a_trusted_hop() {
    let headers = map(&[("forwarded", "proto=https, for=203.0.113.9")]);

    let resolved = Forwarded::resolve(
        &headers,
        Some(peer("10.0.0.1")),
        &TrustedProxies::hops(ProxyHeader::Forwarded, 1),
    );

    assert_eq!(resolved.client(), Some(ip("203.0.113.9")));
    assert_eq!(resolved.proto(), None, "the client's own proto= was read");
    assert_eq!(resolved.client_is_secure(), None);
}

/// The walk stopping at an untrusted sender stops the scheme there too.
#[test]
fn a_scheme_beyond_an_untrusted_sender_is_not_read() {
    let headers = map(&[(
        "forwarded",
        "for=198.51.100.1;proto=https, for=203.0.113.9;proto=http",
    )]);
    let trusted = TrustedProxies::addresses(ProxyHeader::Forwarded, [ip("10.0.0.1")]);

    let resolved = Forwarded::resolve(&headers, Some(peer("10.0.0.1")), &trusted);

    assert_eq!(resolved.client(), Some(ip("203.0.113.9")));
    assert_eq!(resolved.proto(), Some("http"));
}

/// `X-Forwarded-Proto`'s rightmost value is the one the trusted hop wrote.
///
/// A client can send the field itself, and a proxy that appends rather than
/// replaces leaves the client's value leftmost -- on the first line, or on a
/// line of its own before the proxy's. Only the rightmost value, across every
/// line, came from the socket peer.
#[test]
fn the_rightmost_x_forwarded_proto_is_the_trusted_hops() {
    let appended = [("x-forwarded-proto", "https, HTTP")];
    let repeated = [
        ("x-forwarded-proto", "https"),
        ("x-forwarded-proto", "http"),
    ];

    for fields in [&appended[..], &repeated[..]] {
        let mut headers = map(fields);
        headers.append("x-forwarded-for", HeaderValue::from_static("203.0.113.7"));

        let resolved = Forwarded::resolve(
            &headers,
            Some(peer("10.0.0.1")),
            &TrustedProxies::hops(ProxyHeader::XForwarded, 1),
        );

        assert_eq!(resolved.proto(), Some("http"), "{fields:?}");
        assert_eq!(resolved.client_is_secure(), Some(false), "{fields:?}");
    }
}

/// A blank `X-Forwarded-Proto` value adds no element to the list, so the
/// value before it is still the rightmost -- whether the blank is a trailing
/// piece of a line, or a whole line that is empty or only commas.
#[test]
fn a_blank_x_forwarded_proto_value_adds_nothing_to_the_list() {
    let trailing = [("x-forwarded-proto", "http, ")];
    let empty_line = [("x-forwarded-proto", "http"), ("x-forwarded-proto", "")];
    let commas_line = [("x-forwarded-proto", "http"), ("x-forwarded-proto", " , ,")];

    for fields in [&trailing[..], &empty_line[..], &commas_line[..]] {
        let mut headers = map(fields);
        headers.append("x-forwarded-for", HeaderValue::from_static("203.0.113.7"));

        let resolved = Forwarded::resolve(
            &headers,
            Some(peer("10.0.0.1")),
            &TrustedProxies::hops(ProxyHeader::XForwarded, 1),
        );

        assert_eq!(resolved.proto(), Some("http"), "{fields:?}");
    }
}

/// A rightmost `X-Forwarded-Proto` line that is not text, and so cannot be
/// read, does not hand the scheme to the line before it, which may be the
/// client's.
#[test]
fn an_unreadable_rightmost_x_forwarded_proto_states_no_scheme() {
    let mut headers = map(&[
        ("x-forwarded-for", "203.0.113.7"),
        ("x-forwarded-proto", "https"),
    ]);
    headers.append(
        "x-forwarded-proto",
        HeaderValue::from_bytes(b"\xe9").expect("obs-text is a legal field value"),
    );

    let resolved = Forwarded::resolve(
        &headers,
        Some(peer("10.0.0.1")),
        &TrustedProxies::hops(ProxyHeader::XForwarded, 1),
    );

    assert_eq!(resolved.client(), Some(ip("203.0.113.7")));
    assert_eq!(resolved.proto(), None);
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

/// A scheme claimed by a sender nothing trusts is not believed.
///
/// `proto` names no hop of its own -- unlike a `for=` element, which at least
/// says whose address it is -- so the only word behind it is the immediate
/// sender's. A policy naming addresses and meeting a peer outside them has
/// therefore been told nothing it may act on.
///
/// The consequence reaches further than the field. `client_is_secure` is what
/// anything sending HSTS has to consult first, and RFC 6797 section 7.2 forbids
/// that field over non-secure transport -- so believing this claim would put the
/// header on exactly the connection the specification rules out.
///
/// Stated in the conditional because Kynos ships no security-header middleware.
/// This used to link `crate::middleware::security_headers::SecurityHeaders`, a
/// path that resolves to nothing; the link survived only because `cargo doc`
/// does not build a `#[cfg(test)]` module.
#[test]
fn a_scheme_claimed_by_an_untrusted_sender_is_not_believed() {
    let headers = map(&[
        ("x-forwarded-for", "203.0.113.7"),
        ("x-forwarded-proto", "https"),
    ]);
    let trusted = TrustedProxies::addresses(ProxyHeader::XForwarded, [ip("10.0.0.1")]);

    // The peer is not the address the policy names, so nothing it wrote counts.
    let resolved = Forwarded::resolve(&headers, Some(peer("203.0.113.9")), &trusted);

    assert_eq!(resolved.client(), Some(ip("203.0.113.9")));
    assert_eq!(
        resolved.proto(),
        None,
        "a scheme was read from a hop the policy never named"
    );
    assert_eq!(resolved.client_is_secure(), None);
}

/// The control for the case above: the same claim from a sender the policy does
/// name is believed.
///
/// Without it the assertion above passes for a `resolve` that drops every
/// scheme, which would make the field useless rather than trustworthy.
#[test]
fn a_scheme_claimed_by_a_trusted_sender_is_believed() {
    let headers = map(&[
        ("x-forwarded-for", "203.0.113.7"),
        ("x-forwarded-proto", "https"),
    ]);
    let trusted = TrustedProxies::addresses(ProxyHeader::XForwarded, [ip("10.0.0.1")]);

    let resolved = Forwarded::resolve(&headers, Some(peer("10.0.0.1")), &trusted);

    assert_eq!(resolved.client(), Some(ip("203.0.113.7")));
    assert_eq!(resolved.client_is_secure(), Some(true));
}

/// A request that arrived on no socket, with nothing trusted to name one.
#[test]
fn a_request_from_no_socket_resolves_to_no_client() {
    let resolved = Forwarded::resolve(&HeaderMap::new(), None, &TrustedProxies::none());
    assert_eq!(resolved.client(), None);
}

/// Only the router's record answers `of`, and only once the router wrote it.
///
/// Neither a forwarding field nor a `Forwarded` inserted by hand stands in for
/// the resolution: the field is unvetted, and the extension is one the router
/// stopped reading.
#[test]
fn only_a_routed_request_has_a_resolved_origin() {
    let mut request = Request::new(Body::empty());
    request.headers_mut().insert(
        "forwarded",
        HeaderValue::from_static("for=203.0.113.7;proto=https"),
    );
    request.extensions_mut().insert(Forwarded {
        client: Some(ip("203.0.113.8")),
        proto: None,
    });
    assert_eq!(Forwarded::of(&request), None);

    let resolved = Forwarded {
        client: Some(ip("10.0.0.1")),
        proto: None,
    };
    request
        .extensions_mut()
        .insert(crate::router::dispatch::Routed {
            matched: crate::extract::connection::MatchedPath("/origin"),
            captures: None,
            forwarded: resolved.clone(),
        });
    assert_eq!(Forwarded::of(&request), Some(&resolved));
}
