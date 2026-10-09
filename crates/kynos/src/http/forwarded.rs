//! Who sent a request that reached the service through a proxy.
//!
//! Forwarding fields are client-forgeable (RFC 7239 section 8.1), so nothing
//! here reads one unless [`TrustedProxies`] names the hops trusted and the
//! field they write. The default trusts nobody and resolves every request to
//! the socket peer.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::str::FromStr;

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
/// let trusted = TrustedProxies::networks(ProxyHeader::Forwarded, ["10.0.0.0/8".parse()?]);
/// # let _ = trusted;
/// # Ok::<(), kynos::http::forwarded::InvalidNetwork>(())
/// ```
#[derive(Clone, Debug, Default)]
pub struct TrustedProxies {
    /// The field the trusted hops write; `None` reads none.
    header: Option<ProxyHeader>,
    /// How many rightmost elements may be believed.
    hops: usize,
    /// Exact addresses that may be believed, whatever their position.
    addresses: Vec<IpAddr>,
    /// Networks that may be believed.
    networks: Vec<Network>,
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

    /// Trusts every address in these networks, which write `header`.
    #[must_use]
    pub fn networks(header: ProxyHeader, networks: impl IntoIterator<Item = Network>) -> Self {
        Self {
            header: Some(header),
            networks: networks.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Trusts every address in the networks `entries` write as
    /// `address/prefix`, which write `header`.
    ///
    /// For a list read from configuration. Each entry is read as [`Network`]
    /// reads it, so `/0` is refused here too: say
    /// [`everyone`](Self::everyone) instead.
    ///
    /// ```
    /// use kynos::http::forwarded::{ProxyHeader, TrustedProxies};
    ///
    /// let configured = ["10.0.0.0/8", "2001:db8::/32"];
    /// let trusted = TrustedProxies::parse_networks(ProxyHeader::XForwarded, configured)?;
    /// # let _ = trusted;
    ///
    /// let refused = TrustedProxies::parse_networks(ProxyHeader::XForwarded, ["0.0.0.0/0"]);
    /// assert_eq!(refused.unwrap_err().entry(), "0.0.0.0/0");
    /// # Ok::<(), kynos::http::forwarded::InvalidNetworkEntry>(())
    /// ```
    ///
    /// # Errors
    ///
    /// The first entry that is not a [`Network`], and why.
    pub fn parse_networks<I>(header: ProxyHeader, entries: I) -> Result<Self, InvalidNetworkEntry>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let networks = entries
            .into_iter()
            .map(|entry| {
                let entry = entry.as_ref();
                entry.parse().map_err(|reason| InvalidNetworkEntry {
                    entry: entry.to_owned(),
                    reason,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self::networks(header, networks))
    }

    /// Trusts every peer, IPv4 and IPv6, which writes `header`.
    ///
    /// Every element of the field is then believed, the leftmost one included,
    /// and the client writes that one: only right where nothing but trusted
    /// proxies can reach the service. [`Network`] refuses `/0` so that this is
    /// never said by accident.
    #[must_use]
    pub fn everyone(header: ProxyHeader) -> Self {
        let every = [Ipv4Addr::UNSPECIFIED.into(), Ipv6Addr::UNSPECIFIED.into()];
        Self {
            header: Some(header),
            networks: every.map(Network::every).into(),
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
    pub fn and_networks(mut self, networks: impl IntoIterator<Item = Network>) -> Self {
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
                .any(|network| network.contains(address))
    }
}

/// A block of addresses sharing their leading `prefix` bits, written
/// `address/prefix` (RFC 4632 section 3.1, RFC 4291 section 2.3).
///
/// Never `/0`, and never a prefix longer than its address: the first trusts
/// every peer there is, which [`TrustedProxies::everyone`] says outright, and
/// the second matches nothing. Bits past the prefix are cleared, so
/// `10.1.2.3/8` is `10.0.0.0/8`, and that is what [`Display`](fmt::Display)
/// prints.
///
/// A network matches addresses of its own family only: `::ffff:10.0.0.1` is
/// not in `10.0.0.0/8`.
///
/// ```
/// use kynos::http::forwarded::{InvalidNetwork, Network};
///
/// let network: Network = "10.1.2.3/8".parse()?;
/// assert_eq!(network.to_string(), "10.0.0.0/8");
/// assert!(network.contains("10.200.0.1".parse()?));
///
/// assert_eq!("0.0.0.0/0".parse::<Network>(), Err(InvalidNetwork::EveryAddress));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Network {
    /// The first address of the block, its host bits cleared.
    address: IpAddr,
    /// How many leading bits every member shares; never `0` once public.
    prefix: u8,
}

impl Network {
    /// The block of addresses sharing `address`'s leading `prefix` bits.
    ///
    /// # Errors
    ///
    /// [`InvalidNetwork::EveryAddress`] for a `prefix` of `0`, and
    /// [`InvalidNetwork::PrefixTooLong`] for one past the address's 32 or 128
    /// bits.
    pub fn new(address: IpAddr, prefix: u8) -> Result<Self, InvalidNetwork> {
        let bits = if address.is_ipv4() { 32 } else { 128 };
        if prefix == 0 {
            return Err(InvalidNetwork::EveryAddress);
        }
        if prefix > bits {
            return Err(InvalidNetwork::PrefixTooLong { bits });
        }
        Ok(Self::masked(address, prefix))
    }

    /// Every address of `address`'s family, which only
    /// [`TrustedProxies::everyone`] holds.
    fn every(address: IpAddr) -> Self {
        Self::masked(address, 0)
    }

    /// `address`/`prefix` with the bits past `prefix` cleared; `prefix` is at
    /// most `address`'s width.
    fn masked(address: IpAddr, prefix: u8) -> Self {
        let address = match address {
            IpAddr::V4(address) => {
                let mask = u32::MAX.checked_shl(32 - u32::from(prefix)).unwrap_or(0);
                IpAddr::V4((address.to_bits() & mask).into())
            }
            IpAddr::V6(address) => {
                let mask = u128::MAX.checked_shl(128 - u32::from(prefix)).unwrap_or(0);
                IpAddr::V6((address.to_bits() & mask).into())
            }
        };
        Self { address, prefix }
    }

    /// The first address of the block.
    #[must_use]
    pub fn address(&self) -> IpAddr {
        self.address
    }

    /// How many leading bits every address in the block shares.
    #[must_use]
    pub fn prefix(&self) -> u8 {
        self.prefix
    }

    /// Whether `address` is in this block.
    #[must_use]
    pub fn contains(&self, address: IpAddr) -> bool {
        // No v4/v6 mapping, and a v6 prefix is no mask for a v4 address.
        address.is_ipv4() == self.address.is_ipv4()
            && Self::masked(address, self.prefix).address == self.address
    }
}

impl FromStr for Network {
    type Err = InvalidNetwork;

    /// Reads `address/prefix`, with no surrounding whitespace, the prefix in
    /// decimal digits and no leading zero.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (address, prefix) = text.split_once('/').ok_or(InvalidNetwork::NoPrefix)?;
        let address = address
            .parse::<IpAddr>()
            .map_err(|_| InvalidNetwork::Address)?;

        let digits = prefix.bytes().all(|byte| byte.is_ascii_digit());
        if prefix.is_empty() || !digits || (prefix.len() > 1 && prefix.starts_with('0')) {
            return Err(InvalidNetwork::Prefix);
        }
        let prefix = prefix.parse::<u8>().unwrap_or(u8::MAX);

        Self::new(address, prefix)
    }
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.address, self.prefix)
    }
}

/// Why a [`Network`] was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidNetwork {
    /// No `/prefix` follows the address.
    #[error("no `/prefix` follows the address")]
    NoPrefix,
    /// The text before `/` is not an IPv4 or IPv6 address.
    #[error("the text before `/` is not an IPv4 or IPv6 address")]
    Address,
    /// The text after `/` is not a decimal number without a leading zero.
    #[error("the text after `/` is not a decimal number without a leading zero")]
    Prefix,
    /// The prefix is longer than the address, so the block holds nothing.
    #[error("the prefix is longer than the address's {bits} bits")]
    PrefixTooLong {
        /// How many bits the address has: 32 or 128.
        bits: u8,
    },
    /// The prefix is `0`, so the block holds every address of its family.
    #[error(
        "a `/0` network holds every address there is; `TrustedProxies::everyone` trusts every \
         peer explicitly"
    )]
    EveryAddress,
}

/// An entry [`TrustedProxies::parse_networks`] refused, and why.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{entry:?} is not a network to trust: {reason}")]
pub struct InvalidNetworkEntry {
    /// The entry as given.
    entry: String,
    /// Why it is not a [`Network`].
    reason: InvalidNetwork,
}

impl InvalidNetworkEntry {
    /// The entry as given.
    #[must_use]
    pub fn entry(&self) -> &str {
        &self.entry
    }

    /// Why it is not a [`Network`].
    #[must_use]
    pub fn reason(&self) -> InvalidNetwork {
        self.reason
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
