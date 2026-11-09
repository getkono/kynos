//! The PROXY protocol header a load balancer that does not speak HTTP sends
//! ahead of a connection it relays, naming the client it relays for.
//!
//! Versions 1 (text) and 2 (binary), as HAProxy's specification defines them.
//! The header is read before anything else on the socket, TLS included, and
//! whatever arrived past it is handed back first. A version 2 header's TLVs
//! are read past and ignored, and so is a destination address: the
//! connection's local address stays the socket's own.
//!
//! The specification's rejections are kept: a version 1 line past 107 bytes
//! or not ending in CRLF, and in version 2 a version, command, address family
//! or transport it does not list, or an address block shorter than its family
//! needs. Where it says to use the connection's own endpoints — version 1's
//! `UNKNOWN`, version 2's `LOCAL` command, or a family other than IPv4 and
//! IPv6 — the header is accepted and the socket's peer stands.

use std::{
    future::poll_fn,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Buf as _, Bytes};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// The longest version 1 header, its CRLF included.
const V1_MAX: usize = 107;

/// What every version 1 header starts with.
const V1_PREFIX: &[u8] = b"PROXY ";

/// What every version 2 header starts with.
const V2_SIGNATURE: &[u8; 12] = b"\r\n\r\n\0\r\nQUIT\n";

/// A version 2 header's fixed part: the signature, the version and command,
/// the family and transport, and the length of what follows.
const V2_FIXED: usize = 16;

/// What one header said about the connection it precedes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Header {
    /// The proxy's own connection, such as a health check, or one whose
    /// addresses no family read here names: served as the socket's peer.
    Local,
    /// A connection relayed for this client.
    Relayed(SocketAddr),
}

/// How far a buffer holding the start of a connection reads as a header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Parsed {
    /// A header may still follow: read more.
    Partial,
    /// A whole header, and how many bytes it took.
    Complete(Header, usize),
}

/// The connection does not open with a header the specification admits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Malformed;

/// Reads `bytes`, the start of a connection, as a PROXY protocol header of
/// either version.
///
/// Refuses as early as the bytes allow, so a connection that sends no header
/// is closed at its first byte rather than at a deadline.
///
/// # Errors
///
/// [`Malformed`] where no header the specification admits starts `bytes`.
pub(crate) fn parse(bytes: &[u8]) -> Result<Parsed, Malformed> {
    if starts(bytes, V2_SIGNATURE) {
        return if bytes.len() < V2_SIGNATURE.len() {
            Ok(Parsed::Partial)
        } else {
            v2(bytes)
        };
    }
    if starts(bytes, V1_PREFIX) {
        return if bytes.len() < V1_PREFIX.len() {
            Ok(Parsed::Partial)
        } else {
            v1(bytes)
        };
    }
    Err(Malformed)
}

/// Whether `bytes` and `prefix` agree as far as both go.
fn starts(bytes: &[u8], prefix: &[u8]) -> bool {
    let shared = bytes.len().min(prefix.len());
    bytes[..shared] == prefix[..shared]
}

/// A version 1 header: `PROXY`, the protocol, and for `TCP4` and `TCP6` the
/// source and destination addresses and ports, one space apart, then CRLF.
fn v1(bytes: &[u8]) -> Result<Parsed, Malformed> {
    let window = &bytes[..bytes.len().min(V1_MAX)];
    let Some(feed) = window.iter().position(|&byte| byte == b'\n') else {
        return if bytes.len() < V1_MAX {
            Ok(Parsed::Partial)
        } else {
            Err(Malformed)
        };
    };
    // A line ends at its first LF, which must follow a CR.
    if window[feed - 1] != b'\r' {
        return Err(Malformed);
    }
    let end = feed - 1;
    let mut fields = bytes[V1_PREFIX.len()..end].split(|&byte| byte == b' ');
    let header = match fields.next() {
        // Whatever follows up to the CRLF is ignored.
        Some(b"UNKNOWN") => Header::Local,
        Some(b"TCP4") => Header::Relayed(v1_addresses::<Ipv4Addr>(fields)?),
        Some(b"TCP6") => Header::Relayed(v1_addresses::<Ipv6Addr>(fields)?),
        _ => return Err(Malformed),
    };
    Ok(Parsed::Complete(header, end + 2))
}

/// The source of a `TCP4` or `TCP6` line's four remaining fields, each of
/// which must read as its kind, with none past them.
fn v1_addresses<'a, A>(mut fields: impl Iterator<Item = &'a [u8]>) -> Result<SocketAddr, Malformed>
where
    A: std::str::FromStr + Into<IpAddr>,
{
    let mut next = || fields.next().ok_or(Malformed);
    let address = |field: &[u8]| {
        std::str::from_utf8(field)
            .ok()
            .and_then(|text| text.parse::<A>().ok())
            .ok_or(Malformed)
    };
    let source = address(next()?)?;
    address(next()?)?;
    let source_port = port(next()?)?;
    port(next()?)?;
    if fields.next().is_some() {
        return Err(Malformed);
    }
    Ok(SocketAddr::new(source.into(), source_port))
}

/// A port written in decimal digits alone, from 0 to 65535.
fn port(field: &[u8]) -> Result<u16, Malformed> {
    if field.is_empty() || field.len() > 5 || !field.iter().all(u8::is_ascii_digit) {
        return Err(Malformed);
    }
    std::str::from_utf8(field)
        .ok()
        .and_then(|digits| digits.parse().ok())
        .ok_or(Malformed)
}

/// The address family a version 2 `PROXY` command names, and so how its
/// address block is read.
#[derive(Clone, Copy)]
enum Family {
    /// `LOCAL`, or `AF_UNSPEC`: no address is read.
    Unread,
    Ipv4,
    Ipv6,
    /// `AF_UNIX`, whose paths name no client this server can report.
    Unix,
}

impl Family {
    /// How long the address block must be at least.
    const fn least(self) -> usize {
        match self {
            Self::Unread => 0,
            Self::Ipv4 => 12,
            Self::Ipv6 => 36,
            Self::Unix => 216,
        }
    }
}

/// A version 2 header, whose signature `bytes` already holds.
fn v2(bytes: &[u8]) -> Result<Parsed, Malformed> {
    if bytes.len() < V2_FIXED {
        return Ok(Parsed::Partial);
    }
    if bytes[12] >> 4 != 2 {
        return Err(Malformed);
    }
    let family = match bytes[12] & 0x0f {
        // `LOCAL` ignores the family, whatever it says.
        0 => Family::Unread,
        1 => match bytes[13] {
            0x00 => Family::Unread,
            0x11 | 0x12 => Family::Ipv4,
            0x21 | 0x22 => Family::Ipv6,
            0x31 | 0x32 => Family::Unix,
            _ => return Err(Malformed),
        },
        _ => return Err(Malformed),
    };
    let block = usize::from(u16::from_be_bytes([bytes[14], bytes[15]]));
    if block < family.least() {
        return Err(Malformed);
    }
    let length = V2_FIXED + block;
    if bytes.len() < length {
        return Ok(Parsed::Partial);
    }
    let block = &bytes[V2_FIXED..length];
    let header = match family {
        Family::Ipv4 => {
            let source = Ipv4Addr::new(block[0], block[1], block[2], block[3]);
            let port = u16::from_be_bytes([block[8], block[9]]);
            Header::Relayed(SocketAddr::new(source.into(), port))
        }
        Family::Ipv6 => {
            let mut octets = [0; 16];
            octets.copy_from_slice(&block[..16]);
            let port = u16::from_be_bytes([block[32], block[33]]);
            Header::Relayed(SocketAddr::new(Ipv6Addr::from(octets).into(), port))
        }
        Family::Unread | Family::Unix => Header::Local,
    };
    Ok(Parsed::Complete(header, length))
}

/// Reads the header `io` opens with, and hands back `io` with whatever
/// arrived past it still to be read.
///
/// Unbounded in time: the caller races it against the first-head deadline.
pub(in crate::server) async fn receive<I>(mut io: I) -> io::Result<(Header, Rewound<I>)>
where
    I: AsyncRead + Unpin,
{
    // Room for any version 1 header and a version 2 one without TLVs, so one
    // read takes most headers whole.
    let mut buffer = vec![0; 128];
    let mut filled = 0;
    loop {
        match parse(&buffer[..filled]) {
            Ok(Parsed::Complete(header, length)) => {
                buffer.truncate(filled);
                let unread = Bytes::from(buffer).slice(length..);
                return Ok((header, Rewound { unread, io }));
            }
            Ok(Parsed::Partial) => {}
            Err(Malformed) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "no PROXY protocol header the specification admits",
                ));
            }
        }
        if filled == buffer.len() {
            // A partial header is shorter than the longest version 2 one.
            buffer.resize((buffer.len() * 2).min(V2_FIXED + usize::from(u16::MAX)), 0);
        }
        let mut read = ReadBuf::new(&mut buffer[filled..]);
        poll_fn(|context| Pin::new(&mut io).poll_read(context, &mut read)).await?;
        let read = read.filled().len();
        if read == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        filled += read;
    }
}

/// A stream whose first bytes were read with its header, handed back before
/// the rest.
#[derive(Debug)]
pub(in crate::server) struct Rewound<I> {
    unread: Bytes,
    io: I,
}

impl<I: AsyncRead + Unpin> AsyncRead for Rewound<I> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.unread.is_empty() {
            return Pin::new(&mut self.io).poll_read(context, buf);
        }
        let taken = self.unread.len().min(buf.remaining());
        buf.put_slice(&self.unread[..taken]);
        self.unread.advance(taken);
        Poll::Ready(Ok(()))
    }
}

impl<I: AsyncWrite + Unpin> AsyncWrite for Rewound<I> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.io).poll_write(context, buf)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffers: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.io).poll_write_vectored(context, buffers)
    }

    fn is_write_vectored(&self) -> bool {
        self.io.is_write_vectored()
    }

    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(context)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(context)
    }
}
