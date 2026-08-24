//! Who sent a request that reached the service through a proxy.
//!
//! RFC 7239 defines `Forwarded`, and section 8.1 is blunt about what it is
//! worth: the field "cannot be relied upon to be correct, as it may be
//! modified, whether mistakenly or for malicious reasons, by every node on the
//! way to the server, including the client making the request."
//!
//! So nothing here reads it unless the application has said which hops it
//! trusts. [`TrustedProxies`] is that statement, it is empty by default, and an
//! empty one resolves every request to the socket peer — the one address no
//! header can forge.

use std::net::{IpAddr, SocketAddr};

use crate::http::{HeaderMap, HeaderName};

/// The `Forwarded` field, per RFC 7239.
const FORWARDED: HeaderName = HeaderName::from_static("forwarded");

/// The de-facto field `Forwarded` replaced, still what most proxies send.
const X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");

/// The de-facto scheme field, likewise.
const X_FORWARDED_PROTO: HeaderName = HeaderName::from_static("x-forwarded-proto");

/// Which hops may be believed when they describe the client.
///
/// Empty by default, which means "believe nobody": the socket peer is the
/// client, and every forwarding field is ignored. That is the only safe default
/// — the fields are attacker-controlled — and it is the same rule
/// [`Cors::new`](crate::middleware::cors::Cors::new) follows, where every
/// widening is a call a reviewer can see.
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
/// use kynos::http::forwarded::TrustedProxies;
///
/// // One managed load balancer in front of the service.
/// let trusted = TrustedProxies::hops(1);
///
/// // Or a known private range.
/// let trusted = TrustedProxies::networks([("10.0.0.0".parse().unwrap(), 8)]);
/// # let _ = trusted;
/// ```
#[derive(Clone, Debug, Default)]
pub struct TrustedProxies {
    /// How many rightmost elements may be believed.
    hops: usize,
    /// Exact addresses that may be believed, whatever their position.
    addresses: Vec<IpAddr>,
    /// Networks that may be believed, as an address and a prefix length.
    networks: Vec<(IpAddr, u8)>,
}

impl TrustedProxies {
    /// Trusts nobody. The default.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Trusts the `count` hops nearest the service.
    ///
    /// Counted from the right, because the rightmost element was written by the
    /// hop closest to Kynos and each step left is one step further from anything
    /// this deployment controls.
    #[must_use]
    pub fn hops(count: usize) -> Self {
        Self {
            hops: count,
            ..Self::default()
        }
    }

    /// Trusts these exact addresses.
    #[must_use]
    pub fn addresses(addresses: impl IntoIterator<Item = IpAddr>) -> Self {
        Self {
            addresses: addresses.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Trusts every address in these networks, each an address and a prefix
    /// length.
    #[must_use]
    pub fn networks(networks: impl IntoIterator<Item = (IpAddr, u8)>) -> Self {
        Self {
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

    /// Whether this configuration believes anything at all.
    #[must_use]
    pub fn trusts_nobody(&self) -> bool {
        self.hops == 0 && self.addresses.is_empty() && self.networks.is_empty()
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
/// calling — the rate limiter's [`ByClientAddress`] key, and whether a response
/// may carry HSTS.
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
    /// Resolves what `headers` claim, as far as `trusted` permits.
    ///
    /// `peer` is the socket the request actually arrived on, and it is the
    /// answer whenever the fields cannot be believed.
    #[must_use]
    pub fn resolve(
        headers: &HeaderMap,
        peer: Option<SocketAddr>,
        trusted: &TrustedProxies,
    ) -> Self {
        let peer_ip = peer.map(|peer| peer.ip());

        if trusted.trusts_nobody() {
            return Self {
                client: peer_ip,
                proto: None,
            };
        }

        let (addresses, proto) = elements(headers);

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
        // walk is at -- and it is what `hops` is spent against.
        for (believed, address) in addresses.iter().rev().enumerate() {
            let trusted_sender =
                sender.is_some_and(|sender| trusted.names(sender)) || (believed < trusted.hops);
            if !trusted_sender {
                break;
            }

            client = Some(*address);
            sender = Some(*address);
        }

        Self {
            client,
            proto: proto.filter(|_| peer_is_trusted),
        }
    }

    /// The client address, as far as the trust policy could resolve it.
    ///
    /// `None` only where the request arrived on no socket and no trusted hop
    /// named one — a `TestClient`, or a directly driven `Service::call`.
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

/// Every `for=` address a request claims, left to right, and the scheme.
///
/// `Forwarded` wins where present, because it is the specified field and
/// carries the scheme in the same element as the address it belongs to. The
/// `X-Forwarded-*` pair is read only in its absence: those names appear in no
/// specification -- RFC 7239 section 7.1 describes them and defines nothing --
/// and reading both risks pairing one hop's address with another's scheme.
fn elements(headers: &HeaderMap) -> (Vec<IpAddr>, Option<String>) {
    let mut addresses = Vec::new();
    let mut proto = None;

    let mut saw_forwarded = false;
    for value in headers.get_all(FORWARDED) {
        let Ok(value) = value.to_str() else { continue };
        saw_forwarded = true;

        for element in value.split(',') {
            let mut element_address = None;
            for pair in element.split(';') {
                let Some((name, raw)) = pair.split_once('=') else {
                    continue;
                };
                let raw = unquote(raw.trim());

                if name.trim().eq_ignore_ascii_case("for") {
                    element_address = node_address(raw);
                } else if name.trim().eq_ignore_ascii_case("proto") {
                    proto = Some(raw.to_ascii_lowercase());
                }
            }

            if let Some(address) = element_address {
                addresses.push(address);
            }
        }
    }

    if saw_forwarded {
        return (addresses, proto);
    }

    for value in headers.get_all(X_FORWARDED_FOR) {
        let Ok(value) = value.to_str() else { continue };
        for hop in value.split(',') {
            if let Some(address) = node_address(hop.trim()) {
                addresses.push(address);
            }
        }
    }

    let proto = headers
        .get(X_FORWARDED_PROTO)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(',')
                .next()
                .unwrap_or(value)
                .trim()
                .to_ascii_lowercase()
        });

    (addresses, proto)
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
