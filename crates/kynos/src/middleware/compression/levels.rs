//! Compression levels, one type per algorithm.
//!
//! The three algorithms number their levels differently and have their knee in
//! a different place, so each is its own type, each refuses a number its own
//! format does not define, and none of them converts into another.

/// The level gzip is asked for, in the range DEFLATE defines.
///
/// 0 to 9, where 0 stores without compressing and 9 is the slowest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GzipLevel(u32);

impl GzipLevel {
    /// The level used unless one is chosen: 6.
    ///
    /// zlib's own default. The curve is flat above it — 9 costs roughly twice
    /// the CPU of 6 for about one per cent of size — and steep below 4.
    pub const DEFAULT: Self = Self(6);

    /// The lowest level that still compresses.
    ///
    /// What a service under CPU pressure should reach for before turning
    /// compression off.
    pub const FASTEST: Self = Self(1);

    /// The highest level DEFLATE defines.
    pub const BEST: Self = Self(9);

    /// The level `level` names, or `None` if DEFLATE does not define it.
    #[must_use]
    pub const fn new(level: u32) -> Option<Self> {
        if level <= 9 { Some(Self(level)) } else { None }
    }

    /// The level as the number the format defines.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl Default for GzipLevel {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The quality brotli is asked for, in the range RFC 7932 defines.
///
/// 0 to 11, where 11 is the slowest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BrotliLevel(u32);

impl BrotliLevel {
    /// The quality used unless one is chosen: 4.
    ///
    /// **Not** the encoder's own default of 11, which is meant for content
    /// compressed once and served many times: it encodes at roughly one
    /// megabyte a second, a fifth of a second of CPU for a 200 KB document.
    /// 4 beats gzip 6 on size while costing less CPU.
    ///
    /// Raise it for content you generate once. Do not raise it for an API.
    pub const DEFAULT: Self = Self(4);

    /// The lowest quality that still compresses.
    pub const FASTEST: Self = Self(1);

    /// The highest quality RFC 7932 defines.
    ///
    /// The encoder's own default, and appropriate only for content produced
    /// ahead of time. See [`DEFAULT`](BrotliLevel::DEFAULT).
    pub const BEST: Self = Self(11);

    /// The quality `level` names, or `None` if RFC 7932 does not define it.
    #[must_use]
    pub const fn new(level: u32) -> Option<Self> {
        if level <= 11 { Some(Self(level)) } else { None }
    }

    /// The quality as the number the format defines.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl Default for BrotliLevel {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The level zstd is asked for, in the range the reference encoder defines.
///
/// 1 to 22. The negative "fast" levels are not reachable: a response that
/// compresses that badly should be sent as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ZstdLevel(i32);

impl ZstdLevel {
    /// The level used unless one is chosen: 3.
    ///
    /// zstd's own default; it beats gzip 6 on both size and speed. RFC 9659
    /// fixes the window at 8 MB for HTTP, and every level here stays inside it.
    pub const DEFAULT: Self = Self(3);

    /// The lowest level reachable here.
    pub const FASTEST: Self = Self(1);

    /// The highest level the reference encoder defines.
    ///
    /// Levels above about 12 are for archival: they cost memory as well as
    /// time, and the extra memory is per concurrent encode.
    pub const BEST: Self = Self(22);

    /// The level `level` names, or `None` if it is outside 1 to 22.
    #[must_use]
    pub const fn new(level: i32) -> Option<Self> {
        if level >= 1 && level <= 22 {
            Some(Self(level))
        } else {
            None
        }
    }

    /// The level as the number the encoder defines.
    #[must_use]
    pub const fn get(self) -> i32 {
        self.0
    }
}

impl Default for ZstdLevel {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests;
