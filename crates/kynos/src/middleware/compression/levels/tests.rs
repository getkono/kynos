use super::{BrotliLevel, GzipLevel, ZstdLevel};
use crate::middleware::compression::{Compression, Levels};

/// Every boundary of every range, swept rather than sampled: the failure a
/// range check has is off-by-one at exactly one end, and a sample of the
/// middle cannot see it.
#[test]
fn each_format_accepts_the_levels_it_defines_and_no_others() {
    for level in 0..=9 {
        assert!(
            GzipLevel::new(level).is_some(),
            "gzip refused level {level}, which zlib defines"
        );
    }
    assert_eq!(GzipLevel::new(10), None);

    for level in 0..=11 {
        assert!(
            BrotliLevel::new(level).is_some(),
            "brotli refused quality {level}, which the reference encoder defines"
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

/// The numbers `Compression`'s documentation states — gzip 6, brotli 4, zstd
/// 3 — are the levels both of its constructors hand every encode. `new` and
/// `Default` are written out separately, so each is held.
#[test]
fn both_constructors_encode_at_gzip_6_brotli_4_and_zstd_3() {
    let documented = Levels {
        gzip: GzipLevel::new(6).expect("a zlib level"),
        brotli: BrotliLevel::new(4).expect("a brotli quality"),
        zstd: ZstdLevel::new(3).expect("a zstd level"),
    };

    assert_eq!(GzipLevel::DEFAULT.get(), 6);
    assert_eq!(ZstdLevel::DEFAULT.get(), 3);
    assert_eq!(Levels::default(), documented);
    assert_eq!(Compression::new().levels, documented);
    assert_eq!(Compression::<()>::default().levels, documented);
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
