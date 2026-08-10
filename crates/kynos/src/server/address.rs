//! Where a server accepts traffic: configured addresses and pre-bound
//! listeners.
//!
//! Deliberately separate from the public server URLs in the OpenAPI document.
//! A listener address says where this process accepts traffic; a `servers`
//! entry says where clients should call, and the two are rarely the same.

use std::{
    fmt, io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
};

use tokio::net::TcpListener;

use crate::server::error::ServerError;

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

/// A supported pre-bound TCP listener.
///
/// The closed enum preserves Kynos's TCP transport guarantees while accepting
/// either standard-library or Tokio ownership.
#[derive(Debug)]
#[non_exhaustive]
pub enum Listener {
    /// A standard-library listener, converted to nonblocking Tokio ownership.
    Standard(std::net::TcpListener),
    /// A listener already owned by Tokio.
    Tokio(TcpListener),
}

impl From<std::net::TcpListener> for Listener {
    fn from(listener: std::net::TcpListener) -> Self {
        Self::Standard(listener)
    }
}

impl From<TcpListener> for Listener {
    fn from(listener: TcpListener) -> Self {
        Self::Tokio(listener)
    }
}

pub(in crate::server) async fn resolve(
    address: &BindAddress,
) -> std::result::Result<Vec<SocketAddr>, ServerError> {
    let resolved = match &address.0 {
        BindTarget::Socket(address) => return Ok(vec![*address]),
        BindTarget::Name(name) => tokio::net::lookup_host(name.as_str())
            .await
            .map_err(|source| ServerError::Resolve {
                address: address.clone(),
                source,
            })?
            .collect::<Vec<_>>(),
        BindTarget::Host { host, port } => tokio::net::lookup_host((host.as_str(), *port))
            .await
            .map_err(|source| ServerError::Resolve {
                address: address.clone(),
                source,
            })?
            .collect::<Vec<_>>(),
    };
    if resolved.is_empty() {
        return Err(ServerError::Resolve {
            address: address.clone(),
            source: io::Error::new(io::ErrorKind::AddrNotAvailable, "no addresses resolved"),
        });
    }
    Ok(resolved)
}
