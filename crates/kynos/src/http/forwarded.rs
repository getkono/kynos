//! Who sent a request that reached the service through a proxy.
//!
//! Forwarding fields are client-forgeable (RFC 7239 section 8.1), so nothing
//! here reads one unless [`TrustedProxies`] names the hops trusted and the
//! field they write. The default trusts nobody and resolves every request to
//! the socket peer.

use std::net::{IpAddr, SocketAddr};

use crate::http::{HeaderMap, HeaderName};

/// The `Forwarded` field, per RFC 7239.
const FORWARDED: HeaderName = HeaderName::from_static("forwarded");

/// The de-facto field `Forwarded` replaced, still what most proxies send.
const X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");

/// The de-facto scheme field, likewise.
const X_FORWARDED_PROTO: HeaderName = HeaderName::from_static("x-forwarded-proto");

/// The field a trusted proxy writes, and so the only one read.
///
/// A proxy appends to one field and passes the other through from the client
/// (an AWS ALB or typical nginx appends to `X-Forwarded-For`), so reading the
/// wrong one lets the client choose its address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProxyHeader {
    /// `Forwarded`, per RFC 7239, which carries each hop's address and scheme
    /// in one element.
    Forwarded,
    /// The de-facto `X-Forwarded-For` and `X-Forwarded-Proto` pair, which RFC
    /// 7239 section 7.1 describes and no specification defines.
    XForwarded,
}

/// Which hops may be believed when they describe the client, and through which
/// field.
///
/// Empty by default, which means "believe nobody": the socket peer is the
/// client, and every forwarding field is ignored.
///
/// Every constructor that believes anyone names the [`ProxyHeader`] those hops
/// write, and only that field is read.
///
/// # Which constructor
///
/// [`hops`](Self::hops) is right when the number of proxies is fixed and their
/// addresses are not — a managed load balancer whose pool changes under you.
/// [`addresses`](Self::addresses) and [`networks`](Self::networks) are right
/// when you know where the proxies are. They compose: a hop is only counted
/// from an element whose immediate sender was trusted.
///
/// ```
/// use kynos::http::forwarded::{ProxyHeader, TrustedProxies};
///
/// // One managed load balancer in front of the service, appending to
/// // `X-Forwarded-For`.
/// let trusted = TrustedProxies::hops(ProxyHeader::XForwarded, 1);
///
/// // Or a known private range writing `Forwarded`.
/// let trusted =
///     TrustedProxies::networks(ProxyHeader::Forwarded, [("10.0.0.0".parse().unwrap(), 8)]);
/// # let _ = trusted;
/// ```
#[derive(Clone, Debug, Default)]
pub struct TrustedProxies {
    /// The field the trusted hops write; `None` reads none.
    header: Option<ProxyHeader>,
    /// How many rightmost elements may be believed.
    hops: usize,
    /// Exact addresses that may be believed, whatever their position.
    addresses: Vec<IpAddr>,
    /// Networks that may be believed, as an address and a prefix length.
    networks: Vec<(IpAddr, u8)>,
}

impl TrustedProxies {
    /// Trusts nobody. The default.
    ///
    /// It names no [`ProxyHeader`], so nothing [`and_addresses`] or
    /// [`and_networks`] adds to it is ever read from: start from a constructor
    /// that names one.
    ///
    /// [`and_addresses`]: Self::and_addresses
    /// [`and_networks`]: Self::and_networks
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Trusts the `count` hops nearest the service, which write `header`.
    ///
    /// Counted from the right: the rightmost element was written by the hop
    /// closest to the service.
    #[must_use]
    pub fn hops(header: ProxyHeader, count: usize) -> Self {
        Self {
            header: Some(header),
            hops: count,
            ..Self::default()
        }
    }

    /// Trusts these exact addresses, which write `header`.
    #[must_use]
    pub fn addresses(header: ProxyHeader, addresses: impl IntoIterator<Item = IpAddr>) -> Self {
        Self {
            header: Some(header),
            addresses: addresses.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Trusts every address in these networks, each an address and a prefix
    /// length, which write `header`.
    #[must_use]
    pub fn networks(header: ProxyHeader, networks: impl IntoIterator<Item = (IpAddr, u8)>) -> Self {
        Self {
            header: Some(header),
            networks: networks.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Also trusts these exact addresses.
    ///
    /// Adds nothing to a policy that names no [`ProxyHeader`], such as
    /// [`none`](Self::none): that policy still trusts nobody.
    #[must_use]
    pub fn and_addresses(mut self, addresses: impl IntoIterator<Item = IpAddr>) -> Self {
        self.addresses.extend(addresses);
        self
    }

    /// Also trusts every address in these networks.
    ///
    /// Adds nothing to a policy that names no [`ProxyHeader`], such as
    /// [`none`](Self::none): that policy still trusts nobody.
    #[must_use]
    pub fn and_networks(mut self, networks: impl IntoIterator<Item = (IpAddr, u8)>) -> Self {
        self.networks.extend(networks);
        self
    }

    /// The field the trusted hops write, or `None` where nobody is trusted.
    fn header(&self) -> Option<ProxyHeader> {
        self.header.filter(|_| !self.trusts_nobody())
    }

    /// Whether this configuration believes anything at all.
    #[must_use]
    pub fn trusts_nobody(&self) -> bool {
        self.header.is_none()
            || (self.hops == 0 && self.addresses.is_empty() && self.networks.is_empty())
    }

    /// Whether `address` is one of the hops this configuration names.
    fn names(&self, address: IpAddr) -> bool {
        self.addresses.contains(&address)
            || self
                .networks
                .iter()
                .any(|(network, prefix)| within(address, *network, *prefix))
    }
}

/// Whether `address` falls inside `network`/`prefix`.
///
/// Hand-rolled: the default build admits no dependency for it.
fn within(address: IpAddr, network: IpAddr, prefix: u8) -> bool {
    fn matches(address: &[u8], network: &[u8], prefix: u8) -> bool {
        let prefix = usize::from(prefix);
        if prefix > address.len() * 8 {
            return false;
        }

        let (whole, bits) = (prefix / 8, prefix % 8);
        if address[..whole] != network[..whole] {
            return false;
        }
        if bits == 0 {
            return true;
        }

        let mask = 0xffu8 << (8 - bits);
        address[whole] & mask == network[whole] & mask
    }

    match (address, network) {
        (IpAddr::V4(address), IpAddr::V4(network)) => {
            matches(&address.octets(), &network.octets(), prefix)
        }
        (IpAddr::V6(address), IpAddr::V6(network)) => {
            matches(&address.octets(), &network.octets(), prefix)
        }
        // No v4/v6 mapping: `::ffff:10.0.0.1` must not match a `10.0.0.0/8` rule.
        _ => false,
    }
}

/// What a request's forwarding fields say, once the trust policy has been
/// applied.
///
/// Built once per request; read by the rate limiter's [`ByClientAddress`] key,
/// or by a handler taking it as an argument.
///
/// [`ByClientAddress`]: crate::middleware::rate_limit::key::ByClientAddress
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Forwarded {
    /// The client address, resolved as far as trust allows.
    client: Option<IpAddr>,
    /// The scheme the client used, where a trusted hop stated one.
    proto: Option<String>,
}

impl Forwarded {
    /// What the router resolved for `request`, borrowed.
    ///
    /// For code handed a `&Request` rather than an argument list: an
    /// [`Observer`](crate::middleware::Observer), a
    /// [`RateLimitKey`](crate::middleware::rate_limit::key::RateLimitKey), an
    /// interceptor. A handler takes `Forwarded` as an argument instead.
    ///
    /// `None` until the request has been routed — the router resolves it once,
    /// under [`Router::trusted_proxies`](crate::Router::trusted_proxies), before
    /// any interceptor runs.
    #[must_use]
    pub fn of(request: &crate::http::Request) -> Option<&Self> {
        request
            .extensions()
            .get::<crate::router::dispatch::Routed>()
            .map(|routed| &routed.forwarded)
    }

    /// Resolves what `headers` claim, as far as `trusted` permits.
    ///
    /// `peer` is the socket the request actually arrived on, and it is the
    /// answer whenever the fields cannot be believed. An element naming no
    /// address is still a hop: see [`client`](Self::client).
    #[must_use]
    pub fn resolve(
        headers: &HeaderMap,
        peer: Option<SocketAddr>,
        trusted: &TrustedProxies,
    ) -> Self {
        let peer_ip = peer.map(|peer| peer.ip());

        let Some(header) = trusted.header() else {
            return Self {
                client: peer_ip,
                proto: None,
            };
        };

        let (chain, peer_proto) = elements(headers, header);

        // `peer_proto` names no hop, so only the socket peer's trust backs it.
        let peer_is_trusted =
            trusted.hops > 0 || peer_ip.is_some_and(|address| trusted.names(address));

        // Walk right to left from the socket peer, stopping at the first
        // untrusted sender (RFC 7239 section 8.1).
        let mut client = peer_ip;
        let mut sender = peer_ip;
        let mut stop = None;

        // An element naming no address still spends a hop, or the client's own
        // element would slide into the trusted budget.
        for (believed, hop) in chain.iter().rev().enumerate() {
            let trusted_sender =
                sender.is_some_and(|sender| trusted.names(sender)) || (believed < trusted.hops);
            if !trusted_sender {
                break;
            }

            client = hop.address;
            sender = hop.address;
            stop = Some(hop);
        }

        // Only the stop element's `proto=` describes the client's connection;
        // nearer ones name inter-proxy hops, further ones are untrusted.
        let proto = stop
            .and_then(|hop| hop.proto)
            .or(peer_proto.filter(|_| peer_is_trusted));

        Self {
            client,
            proto: proto.map(str::to_ascii_lowercase),
        }
    }

    /// The client address, as far as the trust policy could resolve it.
    ///
    /// `None` where no socket and no trusted hop named one — a `TestClient`, a
    /// driven `Service::call` — or where trust ends on an element naming no
    /// address: `for=unknown`, an obfuscated or unparseable `for=`, or no
    /// `for=` at all, as in a proxy sending only `Forwarded: proto=https`.
    #[must_use]
    pub fn client(&self) -> Option<IpAddr> {
        self.client
    }

    /// The scheme a trusted hop said the client used, lowercased.
    #[must_use]
    pub fn proto(&self) -> Option<&str> {
        self.proto.as_deref()
    }

    /// Whether the client's own connection was secure.
    ///
    /// `Some(false)` is a trusted hop saying it was not; `None` is nobody
    /// having said (a distinction RFC 6797 section 7.2 turns on).
    #[must_use]
    pub fn client_is_secure(&self) -> Option<bool> {
        self.proto.as_deref().map(|proto| proto == "https")
    }
}

/// One element of a forwarding field: the hop a proxy wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Hop<'a> {
    /// The `for=` address, or `None` where the element names none (`unknown`,
    /// an `obfnode`, no `for=`).
    address: Option<IpAddr>,
    /// The `proto=` the same element states, as written.
    proto: Option<&'a str>,
}

/// Every non-empty element in `header`, left to right, and the scheme the
/// immediate sender stated outside any element (`X-Forwarded-Proto` only).
///
/// Never falls back to the other field: that one is the client's.
fn elements(headers: &HeaderMap, header: ProxyHeader) -> (Vec<Hop<'_>>, Option<&str>) {
    match header {
        ProxyHeader::Forwarded => (forwarded_elements(headers), None),
        ProxyHeader::XForwarded => x_forwarded_elements(headers),
    }
}

/// [`elements`] from `Forwarded`, whose scheme sits in the element it belongs
/// to.
fn forwarded_elements(headers: &HeaderMap) -> Vec<Hop<'_>> {
    let mut hops = Vec::new();
    for value in headers.get_all(FORWARDED) {
        let Ok(value) = value.to_str() else { continue };

        let start = hops.len();
        for element in unquoted_rsplit(value, b',') {
            let (mut address, mut proto) = (None, None);
            let pairs = unquoted_rsplit(element, b';').filter_map(|pair| pair.split_once('='));
            for (name, raw) in pairs.map(|(name, raw)| (name, unquote(raw.trim()))) {
                if name.trim().eq_ignore_ascii_case("for") {
                    address = address.or(Some(node_address(raw)));
                } else if name.trim().eq_ignore_ascii_case("proto") {
                    proto = proto.or(Some(raw));
                }
            }
            hops.push(Hop {
                address: address.flatten(),
                proto,
            });
        }
        hops[start..].reverse();
    }

    hops
}

/// [`elements`] from the `X-Forwarded-For` and `X-Forwarded-Proto` pair.
///
/// The scheme is `X-Forwarded-Proto`'s rightmost non-empty list element (RFC
/// 9110 sections 5.3, 5.6.1); a non-text rightmost line yields none rather
/// than an earlier, further-out line's.
fn x_forwarded_elements(headers: &HeaderMap) -> (Vec<Hop<'_>>, Option<&str>) {
    let mut hops = Vec::new();
    for value in headers.get_all(X_FORWARDED_FOR) {
        let Ok(value) = value.to_str() else { continue };
        let addresses = value.split(',').map(str::trim);
        hops.extend(
            addresses
                .filter(|address| !address.is_empty())
                .map(|address| Hop {
                    address: node_address(address),
                    proto: None,
                }),
        );
    }

    let mut proto = None;
    for value in headers.get_all(X_FORWARDED_PROTO).iter().rev() {
        let Ok(value) = value.to_str() else { break };
        proto = value
            .rsplit(',')
            .map(str::trim)
            .find(|value| !value.is_empty());
        if proto.is_some() {
            break;
        }
    }

    (hops, proto)
}

/// `text`'s non-blank pieces between `delimiter`s outside any `quoted-string`
/// (an odd run of `\` escapes its `"`), last first, so a client's unclosed
/// quote cannot swallow a hop's. Allocation-free: it runs behind every proxy.
fn unquoted_rsplit(text: &str, delimiter: u8) -> impl Iterator<Item = &str> {
    let (bytes, mut end) = (text.as_bytes(), Some(text.len()));
    let pieces = std::iter::from_fn(move || {
        let (stop, mut quoted) = (end?, false);
        end = (0..stop).rev().find(|&at| {
            if bytes[at] == b'"' {
                let escapes = bytes[..at].iter().rev().take_while(|&&b| b == b'\\');
                quoted ^= !quoted || escapes.count() % 2 == 0;
            }
            !quoted && bytes[at] == delimiter
        });
        Some(&text[end.map_or(0, |at| at + 1)..stop])
    });
    pieces.filter(|piece| !piece.trim().is_empty())
}

/// Strips one layer of `quoted-string` quoting.
fn unquote(text: &str) -> &str {
    text.strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .unwrap_or(text)
}

/// The address a `node` identifier names, where it names one.
///
/// RFC 7239 section 6; `unknown` and an `obfnode` yield `None`.
fn node_address(node: &str) -> Option<IpAddr> {
    let node = node.trim();

    if let Some(rest) = node.strip_prefix('[') {
        let (address, _) = rest.split_once(']')?;
        return address.parse().ok();
    }

    // Bare IPv6 is ungrammatical but `X-Forwarded-For` proxies send it, so try
    // it before splitting off a port.
    if let Ok(address) = node.parse::<IpAddr>() {
        return Some(address);
    }

    node.split_once(':')
        .map_or(node, |(address, _)| address)
        .parse()
        .ok()
}

#[cfg(test)]
#[path = "forwarded/tests.rs"]
mod tests;
