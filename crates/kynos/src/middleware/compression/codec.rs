//! The codec layer both directions share: the codings, the levels each is
//! asked for, and the drain that reads an encoder or a decoder to its end.
//!
//! [`decompression`](crate::middleware::decompression) decodes through
//! [`drain`] too, so none of this is response-specific.

use std::{io, pin::Pin, task::Poll};

use async_compression::{
    Level,
    tokio::bufread::{BrotliEncoder, GzipEncoder, ZstdEncoder},
    zstd::CParameter,
};
use bytes::{Bytes, BytesMut};
use tokio::io::{AsyncRead, ReadBuf};

use crate::middleware::compression::levels::{BrotliLevel, GzipLevel, ZstdLevel};

/// A content coding this crate can produce and decode, ordered by server
/// preference to break ties between equally weighted codings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Coding {
    Zstd,
    Brotli,
    Gzip,
}

impl Coding {
    /// Every coding, most preferred first.
    pub(super) const ALL: [Self; 3] = [Self::Zstd, Self::Brotli, Self::Gzip];

    /// Every coding's token, in [`ALL`](Coding::ALL)'s order.
    pub(super) const TOKENS: [&'static str; 3] = [
        Self::ALL[0].token(),
        Self::ALL[1].token(),
        Self::ALL[2].token(),
    ];

    /// The token this coding is named by on the wire.
    pub(super) const fn token(self) -> &'static str {
        match self {
            Self::Zstd => "zstd",
            Self::Brotli => "br",
            Self::Gzip => "gzip",
        }
    }

    /// The coding `token` names, case-insensitively (RFC 9110 section 8.4.1),
    /// accepting a deprecated alias such as `x-gzip` (section 8.4.1.3).
    pub(crate) fn from_token(token: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|coding| crate::http::coding::names(token, coding.token()))
    }
}

/// How much of a codec's output one read takes.
///
/// Mirrored by `DRAIN_CHUNK` in `tests/alloc_codecs.rs`; change both together.
const DRAIN_CHUNK: usize = 8 * 1024;

/// Why [`drain`] stopped before its source ended.
#[derive(Debug)]
pub(crate) enum DrainError {
    /// The source failed.
    Read(io::Error),
    /// The output would have passed the cap.
    PastCap,
}

impl From<DrainError> for io::Error {
    /// The read's own failure; `PastCap` arises only under a cap, which
    /// encoding never sets.
    fn from(error: DrainError) -> Self {
        match error {
            DrainError::Read(error) => error,
            DrainError::PastCap => io::ErrorKind::FileTooLarge.into(),
        }
    }
}

/// Reads an encoder or a decoder to its end, by hand so neither direction
/// needs anything of tokio beyond `AsyncRead`.
///
/// With a `cap`, refuses to produce more than that many bytes. Checked per
/// chunk, so a decompression bomb is refused while still small.
pub(crate) async fn drain<R: AsyncRead + Unpin>(
    mut source: R,
    cap: Option<u64>,
) -> Result<Bytes, DrainError> {
    let mut drained = BytesMut::new();
    let mut chunk = [0_u8; DRAIN_CHUNK];

    loop {
        let read = std::future::poll_fn(|context| {
            let mut buffer = ReadBuf::new(&mut chunk);
            std::task::ready!(Pin::new(&mut source).poll_read(context, &mut buffer))?;
            Poll::Ready(io::Result::Ok(buffer.filled().len()))
        })
        .await
        .map_err(DrainError::Read)?;

        if read == 0 {
            return Ok(drained.freeze());
        }

        if let Some(cap) = cap {
            let so_far = u64::try_from(drained.len()).unwrap_or(u64::MAX);
            if so_far.saturating_add(u64::try_from(read).unwrap_or(u64::MAX)) > cap {
                return Err(DrainError::PastCap);
            }
        }

        drained.extend_from_slice(&chunk[..read]);
    }
}

/// Applies `coding` to `bytes` at the level `levels` sets for it.
///
/// Each drain is boxed: its 8 KiB buffer and the encoder's state come to
/// kilobytes, and inline they would widen the interceptor's future, which is
/// held for the whole exchange, on every response it declines too.
pub(super) async fn encode(coding: Coding, bytes: Bytes, levels: Levels) -> io::Result<Bytes> {
    let encoded = match coding {
        Coding::Zstd => {
            Box::pin(drain(
                ZstdEncoder::with_quality_and_params(
                    io::Cursor::new(bytes),
                    levels.for_coding(coding),
                    zstd_window(levels.zstd).as_slice(),
                ),
                None,
            ))
            .await
        }
        Coding::Brotli => {
            Box::pin(drain(
                BrotliEncoder::with_quality(io::Cursor::new(bytes), levels.for_coding(coding)),
                None,
            ))
            .await
        }
        Coding::Gzip => {
            Box::pin(drain(
                GzipEncoder::with_quality(io::Cursor::new(bytes), levels.for_coding(coding)),
                None,
            ))
            .await
        }
    };

    encoded.map_err(io::Error::from)
}

/// The window cap zstd needs beside `level`, if it needs one.
///
/// RFC 9659 section 3: an HTTP encoder "MUST NOT generate frames requiring a
/// `Window_Size` larger than 8 MB". On an input of unknown size, which both
/// paths hand it, zstd's own table gives levels 20 to 22 a 32 to 128 MB
/// window, so theirs is capped at 2^23. Levels up to 19 already stay within
/// it and keep the smaller window they choose: capping every level at 2^23
/// would raise the default's 2 MB fourfold, per concurrent encode.
pub(super) fn zstd_window(level: ZstdLevel) -> Option<CParameter> {
    /// The highest level whose own window fits in 8 MB.
    const WITHIN_WINDOW: i32 = 19;
    /// 8 MB, as the base-2 logarithm zstd takes.
    const HTTP_WINDOW_LOG: u32 = 23;

    (level.get() > WITHIN_WINDOW).then(|| CParameter::window_log(HTTP_WINDOW_LOG))
}

/// The level as `async-compression` spells one. The fallback is unreachable:
/// both levels reaching here are bounded at 11.
fn as_level(level: u32) -> i32 {
    i32::try_from(level).unwrap_or(i32::MAX)
}

/// What each algorithm is asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Levels {
    /// What gzip is asked for.
    pub(crate) gzip: GzipLevel,
    /// What brotli is asked for.
    pub(crate) brotli: BrotliLevel,
    /// What zstd is asked for.
    pub(crate) zstd: ZstdLevel,
}

impl Levels {
    /// What `coding` is asked for, as `async-compression` spells a level.
    ///
    /// zstd's window cap is [`zstd_window`]'s, passed beside it.
    pub(super) fn for_coding(self, coding: Coding) -> Level {
        Level::Precise(match coding {
            Coding::Zstd => self.zstd.get(),
            Coding::Brotli => as_level(self.brotli.get()),
            Coding::Gzip => as_level(self.gzip.get()),
        })
    }
}
