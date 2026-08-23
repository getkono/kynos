//! Compression levels, one type per algorithm.
//!
//! Not abstracted into a shared `Fastest`/`Best` scale, deliberately. The three
//! algorithms number their levels differently, mean different things by them,
//! and have their knee in a different place — brotli 11 is roughly a thousand
//! times slower than brotli 4 for a few per cent of size, while gzip 9 is
//! perhaps twice gzip 6. A shared scale hides exactly the fact an operator
//! needs, and makes "level 5" mean three unrelated things.
//!
//! So each is its own type, each refuses a number its own format does not
//! define, and none of them converts into another.

/// The level gzip is asked for, in the range DEFLATE defines.
///
/// 0 to 9, where 0 stores without compressing and 9 is the slowest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GzipLevel(u32);

impl GzipLevel {
    /// The level used unless one is chosen: 6.
    ///
    /// zlib's own default, and the level essentially every HTTP stack has
    /// shipped for thirty years. The curve is flat above it — 9 costs roughly
    /// twice the CPU of 6 for about one per cent of size — and steep below 4.
    pub const DEFAULT: Self = Self(6);

    /// The lowest level that still compresses.
    ///
    /// What nginx ships as `gzip_comp_level` and what a service under CPU
    /// pressure should reach for before it reaches for turning compression off.
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
    /// **Not** the encoder's own default, and that is the point. Brotli's
    /// reference default is 11, which is meant for content compressed once and
    /// served a million times — a font, a bundle, anything with a build step.
    /// Applied to a response generated per request it is catastrophic: quality
    /// 11 encodes at roughly one megabyte a second, so a 200 KB JSON document
    /// spends a fifth of a second of CPU before a byte of it is sent.
    ///
    /// 4 is what large edge networks serve dynamic content at. It beats gzip 6
    /// on size while costing less CPU, which is the whole reason to prefer
    /// brotli for a response nobody cached.
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
/// 1 to 22. The negative "fast" levels are deliberately not reachable: they
/// trade ratio for speed past the point where the coding is worth negotiating
/// at all, and a response that compresses that badly should be sent as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ZstdLevel(i32);

impl ZstdLevel {
    /// The level used unless one is chosen: 3.
    ///
    /// zstd's own default, and unusually one that suits a server: it beats
    /// gzip 6 on both size and speed, which is why zstd is worth offering at
    /// all. RFC 9659 fixes the window at 8 MB for HTTP, and every level here
    /// stays inside it.
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
mod tests {
    use super::{BrotliLevel, GzipLevel, ZstdLevel};

    /// Every boundary of every range, swept rather than sampled: the failure a
    /// range check has is off-by-one at exactly one end, and a sample of the
    /// middle cannot see it.
    #[test]
    fn each_format_accepts_the_levels_it_defines_and_no_others() {
        for level in 0..=9 {
            assert!(
                GzipLevel::new(level).is_some(),
                "gzip refused level {level}, which DEFLATE defines"
            );
        }
        assert_eq!(GzipLevel::new(10), None);

        for level in 0..=11 {
            assert!(
                BrotliLevel::new(level).is_some(),
                "brotli refused quality {level}, which RFC 7932 defines"
            );
        }
        assert_eq!(BrotliLevel::new(12), None);

        for level in 1..=22 {
            assert!(
                ZstdLevel::new(level).is_some(),
                "zstd refused level {level}, which the encoder defines"
            );
        }
        assert_eq!(ZstdLevel::new(0), None, "zstd numbers its levels from one");
        assert_eq!(ZstdLevel::new(23), None);
        assert_eq!(
            ZstdLevel::new(-1),
            None,
            "the negative fast levels are deliberately unreachable"
        );
    }

    /// A named constant outside its own range would be a level the encoder
    /// rejects, reachable without ever calling the checked constructor.
    #[test]
    fn every_named_level_is_one_its_own_constructor_would_have_accepted() {
        for level in [GzipLevel::FASTEST, GzipLevel::DEFAULT, GzipLevel::BEST] {
            assert_eq!(GzipLevel::new(level.get()), Some(level));
        }

        for level in [
            BrotliLevel::FASTEST,
            BrotliLevel::DEFAULT,
            BrotliLevel::BEST,
        ] {
            assert_eq!(BrotliLevel::new(level.get()), Some(level));
        }

        for level in [ZstdLevel::FASTEST, ZstdLevel::DEFAULT, ZstdLevel::BEST] {
            assert_eq!(ZstdLevel::new(level.get()), Some(level));
        }
    }

    /// The default a builder gets and the documented default are one fact. Two
    /// would drift, and the drift would be invisible: both compile.
    #[test]
    fn the_default_is_the_documented_one() {
        assert_eq!(GzipLevel::default(), GzipLevel::DEFAULT);
        assert_eq!(BrotliLevel::default(), BrotliLevel::DEFAULT);
        assert_eq!(ZstdLevel::default(), ZstdLevel::DEFAULT);
    }

    /// Brotli's default is the one worth asserting outright, because it is the
    /// one that departs from the encoder's own. Quality 11 is for content with
    /// a build step; applied per request it costs a fifth of a second of CPU on
    /// a document a client waits for.
    #[test]
    fn brotli_does_not_inherit_the_reference_encoders_default() {
        assert_ne!(BrotliLevel::DEFAULT, BrotliLevel::BEST);
        assert_eq!(BrotliLevel::DEFAULT.get(), 4);
    }
}
