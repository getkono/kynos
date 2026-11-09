//! Where a server accepts traffic: the addresses it resolves and binds itself,
//! on one socket each or, on Linux and Android, on one per accept shard.
//! A listener bound elsewhere is handed over as the standard library's own
//! type, through [`Server::listener`](crate::server::Server::listener).
//!
//! Deliberately separate from the public server URLs in the OpenAPI document.
//! A listener address says where this process accepts traffic; a `servers`
//! entry says where clients should call, and the two are rarely the same.

use std::{
    fmt, io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
};

use crate::{extract::connection::TransportAddress, server::error::ServerError};

/// An owned address resolved when a server is prepared.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BindAddress(BindTarget);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum BindTarget {
    Socket(SocketAddr),
    Name(String),
    Host { host: String, port: u16 },
}

impl fmt::Display for BindAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            BindTarget::Socket(address) => address.fmt(formatter),
            BindTarget::Name(address) => formatter.write_str(address),
            BindTarget::Host { host, port } => write!(formatter, "{host}:{port}"),
        }
    }
}

impl From<SocketAddr> for BindAddress {
    fn from(address: SocketAddr) -> Self {
        Self(BindTarget::Socket(address))
    }
}

impl From<(IpAddr, u16)> for BindAddress {
    fn from((address, port): (IpAddr, u16)) -> Self {
        SocketAddr::new(address, port).into()
    }
}

impl From<(Ipv4Addr, u16)> for BindAddress {
    fn from((address, port): (Ipv4Addr, u16)) -> Self {
        (IpAddr::V4(address), port).into()
    }
}

impl From<(Ipv6Addr, u16)> for BindAddress {
    fn from((address, port): (Ipv6Addr, u16)) -> Self {
        (IpAddr::V6(address), port).into()
    }
}

impl From<String> for BindAddress {
    fn from(address: String) -> Self {
        Self(BindTarget::Name(address))
    }
}

impl From<&str> for BindAddress {
    fn from(address: &str) -> Self {
        address.to_owned().into()
    }
}

impl From<(String, u16)> for BindAddress {
    fn from((host, port): (String, u16)) -> Self {
        Self(BindTarget::Host { host, port })
    }
}

impl From<(&str, u16)> for BindAddress {
    fn from((host, port): (&str, u16)) -> Self {
        (host.to_owned(), port).into()
    }
}

/// A transport address as a log line or an error message shows it: an IP one
/// as its socket address, a Unix one as its path, `@` and its escaped name
/// where it is abstract, or `(unnamed)`.
///
/// Private, since a Unix path need not be UTF-8 and showing one is lossy: a
/// caller that needs the address matches on it.
pub(in crate::server) struct Shown<'a>(pub(in crate::server) &'a TransportAddress);

impl fmt::Display for Shown<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            TransportAddress::Ip(address) => address.fmt(formatter),
            #[cfg(unix)]
            TransportAddress::Unix(address) => {
                let address = address.as_std();
                if let Some(path) = address.as_pathname() {
                    return path.display().fmt(formatter);
                }
                #[cfg(any(target_os = "linux", target_os = "android"))]
                {
                    #[cfg(target_os = "android")]
                    use std::os::android::net::SocketAddrExt as _;
                    #[cfg(target_os = "linux")]
                    use std::os::linux::net::SocketAddrExt as _;

                    if let Some(name) = address.as_abstract_name() {
                        return write!(formatter, "@{}", name.escape_ascii());
                    }
                }
                formatter.write_str("(unnamed)")
            }
        }
    }
}

pub(in crate::server) async fn resolve(
    address: &BindAddress,
) -> std::result::Result<Vec<SocketAddr>, ServerError> {
    let failed = |source| ServerError::Resolve {
        address: address.clone(),
        source,
    };
    let resolved = match &address.0 {
        BindTarget::Socket(address) => return Ok(vec![*address]),
        BindTarget::Name(name) => tokio::net::lookup_host(name.as_str())
            .await
            .map_err(failed)?
            .collect::<Vec<_>>(),
        BindTarget::Host { host, port } => tokio::net::lookup_host((host.as_str(), *port))
            .await
            .map_err(failed)?
            .collect::<Vec<_>>(),
    };
    if resolved.is_empty() {
        return Err(failed(io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "no addresses resolved",
        )));
    }
    Ok(resolved)
}

/// The listen backlog of each accept shard: the one `tokio::net::TcpListener::bind`
/// gives an unsharded address, which is the standard library's.
#[cfg(any(target_os = "linux", target_os = "android"))]
const SHARD_BACKLOG: u32 = 128;

/// Binds `address` once per shard, every socket with `SO_REUSEPORT`, and
/// returns the address they share with them.
///
/// The first socket settles a zero port and the rest join it, so the shards
/// are one address the kernel spreads connections across, not several. The
/// first failure drops every socket already bound.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(in crate::server) fn bind_shards(
    address: SocketAddr,
    shards: std::num::NonZeroUsize,
) -> std::result::Result<(SocketAddr, Vec<tokio::net::TcpListener>), ServerError> {
    let failed = |address| move |source| ServerError::Bind { address, source };
    let first = bind_shard(address).map_err(failed(address))?;
    let shared = first.local_addr().map_err(failed(address))?;
    let mut listeners = Vec::with_capacity(shards.get());
    listeners.push(first);
    for _ in 1..shards.get() {
        listeners.push(bind_shard(shared).map_err(failed(shared))?);
    }
    Ok((shared, listeners))
}

/// One accept shard, bound as `tokio::net::TcpListener::bind` binds an
/// unsharded address but for `SO_REUSEPORT`.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn bind_shard(address: SocketAddr) -> io::Result<tokio::net::TcpListener> {
    let socket = if address.is_ipv4() {
        tokio::net::TcpSocket::new_v4()
    } else {
        tokio::net::TcpSocket::new_v6()
    }?;
    socket.set_reuseaddr(true)?;
    socket.set_reuseport(true)?;
    socket.bind(address)?;
    socket.listen(SHARD_BACKLOG)
}
