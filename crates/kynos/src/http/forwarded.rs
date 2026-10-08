//! Who sent a request that reached the service through a proxy.
//!
//! RFC 7239 defines `Forwarded`, and section 8.1 is blunt about what it is
//! worth: the field "cannot be relied upon to be correct, as it may be
//! modified, whether mistakenly or for malicious reasons, by every node on the
//! way to the server, including the client making the request."
//!
//! So nothing here reads it unless the application has said which hops it
//! trusts and which field they write. [`TrustedProxies`] is that statement, it
//! is empty by default, and an empty one resolves every request to the socket
//! peer — the one address no header can forge.

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
/// A proxy appends to one of the two and passes the other through as the client
/// sent it — an AWS ALB or a typical nginx appends to `X-Forwarded-For` and
/// leaves a client's `Forwarded` alone. Whichever field the proxy does not
/// write is the client's own word, so reading it lets the client choose its
/// address. That is why no field is read by default, and why the choice is the
/// deployment's rather than the request's.
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
/// client, and every forwarding field is ignored. That is the only safe default
/// — the fields are attacker-controlled — and it is the same rule
/// [`Cors::new`](crate::middleware::cors::Cors::new) follows, where every
/// widening is a call a reviewer can see.
///
/// Every constructor that believes anyone names the [`ProxyHeader`] those hops
/// write, and only that field is read. Neither field is a safe guess: the one
/// the proxy does not write is passed through from the client.
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
    /// Counted from the right, because the rightmost element was written by the
    /// hop closest to Kynos and each step left is one step further from anything
    /// this deployment controls.
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
    #[must_use]
    pub fn and_addresses(mut self, addresses: impl IntoIterator<Item = IpAddr>) -> Self {
        self.addresses.extend(addresses);
        self
    }

    /// Also trusts every address in these networks.
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
/// Hand-rolled rather than taken from a crate, for the reason
/// [`base64`](crate::security) is: this is reachable in the default build, and
/// `architecture.md` admits no dependency there. Comparing whole octets and
/// then the partial one is the whole of it.
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
        // A v4 address is never inside a v6 network or the reverse. Mapping one
        // onto the other would make `::ffff:10.0.0.1` match a `10.0.0.0/8` rule
        // its author never wrote.
        _ => false,
    }
}

/// What a request's forwarding fields say, once the trust policy has been
/// applied.
///
/// Built once per request and read by anything that needs to know who is
/// calling — the rate limiter's [`ByClientAddress`] key, or a handler taking it
/// as an argument.
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

        let (addresses, proto) = elements(headers, header);

        // Whether the hop that wrote the fields may be believed at all. The
        // socket peer is the only sender this process observed rather than was
        // told about, so nothing in the request is worth reading unless that
        // peer is named -- either outright, or by `hops` budgeting a first step
        // of trust. It is what decides `proto`, which names no hop of its own
        // and so has only the immediate sender's word behind it.
        let peer_is_trusted =
            trusted.hops > 0 || peer_ip.is_some_and(|address| trusted.names(address));

        // Walk right to left. The rightmost element was written by the hop
        // nearest Kynos, and its immediate sender is the socket peer; each step
        // left moves one hop further out, and stops the moment a sender is one
        // this deployment does not trust. Section 8.1's first weakness -- "the
        // chain of IP addresses listed before the request came to the proxy
        // cannot be trusted" -- is exactly what stopping there respects.
        let mut client = peer_ip;
        let mut sender = peer_ip;

        // `believed` counts the elements already taken, so it is the index the
        // walk is at -- and it is what `hops` is spent against. One naming no
        // address spends its hop too, or the client's own would slide into it.
        for (believed, address) in addresses.iter().rev().enumerate() {
            let trusted_sender =
                sender.is_some_and(|sender| trusted.names(sender)) || (believed < trusted.hops);
            if !trusted_sender {
                break;
            }

            client = *address;
            sender = *address;
        }

        Self {
            client,
            proto: proto.filter(|_| peer_is_trusted),
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
    /// having said. The three are kept apart because RFC 6797 section 7.2 turns
    /// on the difference: an HSTS host must not send the field over non-secure
    /// transport, so "unknown" and "no" have to lead to the same silence for
    /// different reasons.
    #[must_use]
    pub fn client_is_secure(&self) -> Option<bool> {
        self.proto.as_deref().map(|proto| proto == "https")
    }
}

/// Every non-empty element's `for=` address in `header`, left to right, and the
/// scheme. An element naming none (`unknown`, an `obfnode`, no `for=`) is a
/// `None` hop.
///
/// Only `header` is read. The other field is whatever the client sent, and a
/// fallback to it where `header` is absent would hand the client the address
/// whenever the proxy wrote nothing.
fn elements(headers: &HeaderMap, header: ProxyHeader) -> (Vec<Option<IpAddr>>, Option<String>) {
    match header {
        ProxyHeader::Forwarded => forwarded_elements(headers),
        ProxyHeader::XForwarded => x_forwarded_elements(headers),
    }
}

/// [`elements`] from `Forwarded`, whose scheme sits in the element it belongs
/// to.
fn forwarded_elements(headers: &HeaderMap) -> (Vec<Option<IpAddr>>, Option<String>) {
    let (mut addresses, mut proto) = (Vec::new(), None);
    for value in headers.get_all(FORWARDED) {
        let Ok(value) = value.to_str() else { continue };

        let (start, mut line_proto) = (addresses.len(), None);
        for element in unquoted_rsplit(value, b',') {
            let mut element_address = None;
            let pairs = unquoted_rsplit(element, b';').filter_map(|pair| pair.split_once('='));
            for (name, raw) in pairs.map(|(name, raw)| (name, unquote(raw.trim()))) {
                if name.trim().eq_ignore_ascii_case("for") {
                    element_address = element_address.or(Some(node_address(raw)));
                } else if name.trim().eq_ignore_ascii_case("proto") {
                    line_proto = line_proto.or(Some(raw));
                }
            }
            addresses.push(element_address.flatten());
        }
        addresses[start..].reverse();
        proto = line_proto.or(proto);
    }

    (addresses, proto.map(str::to_ascii_lowercase))
}

/// [`elements`] from the `X-Forwarded-For` and `X-Forwarded-Proto` pair.
fn x_forwarded_elements(headers: &HeaderMap) -> (Vec<Option<IpAddr>>, Option<String>) {
    let mut addresses = Vec::new();
    for value in headers.get_all(X_FORWARDED_FOR) {
        let Ok(value) = value.to_str() else { continue };
        let hops = value.split(',').map(str::trim);
        addresses.extend(hops.filter(|hop| !hop.is_empty()).map(node_address));
    }

    let proto = headers
        .get(X_FORWARDED_PROTO)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(|first| first.trim().to_ascii_lowercase());

    (addresses, proto)
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
/// RFC 7239 section 6: `nodename = IPv4address / "[" IPv6address "]" /
/// "unknown" / obfnode`, each optionally followed by `":" node-port`. Only the
/// first two are addresses; `unknown` and an `obfnode` deliberately are not,
/// and yield `None` rather than a guess.
fn node_address(node: &str) -> Option<IpAddr> {
    let node = node.trim();

    if let Some(rest) = node.strip_prefix('[') {
        let (address, _) = rest.split_once(']')?;
        return address.parse().ok();
    }

    // A bare IPv6 address is outside the grammar -- ":" is not a `token`
    // character, so it must be bracketed -- but `X-Forwarded-For` has no
    // grammar and proxies do send one, so try it before splitting on a colon.
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
