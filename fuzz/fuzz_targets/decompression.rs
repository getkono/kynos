//! The bounded decode `Decompression` runs over a request body.
//!
//! The input's first byte picks a `Content-Encoding`, its second whether the
//! rest is encoded under it here and whether a stray byte follows the encoded
//! stream, the next two the layer's limit and the next its `max_ratio`, zero
//! for none; the rest is the payload.
//!
//! Whatever arrives, what the layer hands on for a coded body is never longer
//! than its limit, nor than `max_ratio` times what arrived, and an uncoded body
//! is handed on as it arrived. A payload encoded here by an independent
//! encoder decodes back to itself, unless a layer's output would pass that
//! bound, which is 413; and a byte after the encoded stream is 400, since a
//! coded body holds nothing after its coding ends.

#![no_main]

use std::io::Write;

use kynos::{
    __private::fuzz,
    http::{HeaderMap, HeaderValue, header::CONTENT_ENCODING},
    middleware::decompression::Reason,
};
use libfuzzer_sys::fuzz_target;

/// A coding the layer decodes.
#[derive(Clone, Copy, Debug)]
enum Coding {
    Gzip,
    Brotli,
    Zstd,
}

/// Each `Content-Encoding` the target sends, and the codings it names in the
/// order they were applied, or `None` for one the layer must refuse.
const FIELDS: &[(&str, Option<&[Coding]>)] = &[
    ("gzip", Some(&[Coding::Gzip])),
    ("x-gzip", Some(&[Coding::Gzip])),
    ("br", Some(&[Coding::Brotli])),
    ("zstd", Some(&[Coding::Zstd])),
    ("GZIP, identity", Some(&[Coding::Gzip])),
    ("gzip, br", Some(&[Coding::Gzip, Coding::Brotli])),
    ("zstd, gzip", Some(&[Coding::Zstd, Coding::Gzip])),
    (
        "br, zstd, gzip, br",
        Some(&[Coding::Brotli, Coding::Zstd, Coding::Gzip, Coding::Brotli]),
    ),
    ("identity", Some(&[])),
    ("", Some(&[])),
    ("compress", None),
    ("gzip, gzip, gzip, gzip, gzip", None),
];

fuzz_target!(|data: &[u8]| {
    let [field, flags, high, low, ratio, payload @ ..] = data else {
        return;
    };
    let (field, codings) = FIELDS[usize::from(*field) % FIELDS.len()];
    let encode = flags & 1 != 0;
    let stray = flags & 2 != 0;
    let limit = u64::from(u16::from_be_bytes([*high, *low]));
    let max_ratio = (*ratio != 0).then_some(u64::from(*ratio));

    // What each layer of decoding should produce, outermost first.
    let mut layers = vec![payload.to_vec()];
    if encode {
        for &coding in codings.unwrap_or_default() {
            let inner = layers.last().expect("the payload");
            layers.push(encoded(coding, inner));
        }
    }
    let mut arrived = layers.pop().expect("the payload");
    layers.reverse();
    if stray {
        arrived.push(0);
    }

    // A body past the limit is refused as it is read, before any decoding.
    let length = u64::try_from(arrived.len()).expect("a fuzz input's length");
    if length > limit {
        return;
    }

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_ENCODING, HeaderValue::from_static(field));
    let handed = fuzz::decompress(&headers, &arrived, limit, max_ratio);

    let Some(codings) = codings else {
        assert_eq!(handed, Err(Reason::UnsupportedCoding), "{field:?}");
        return;
    };
    if codings.is_empty() {
        assert_eq!(handed.as_ref(), Ok(&arrived), "{field:?}");
        return;
    }

    let bound = max_ratio.map_or(limit, |ratio| limit.min(length.saturating_mul(ratio)));
    if let Ok(decoded) = &handed {
        let decoded = u64::try_from(decoded.len()).expect("a decoded length");
        assert!(decoded <= bound, "{field:?} decoded {decoded} past {bound}");
    }
    if !encode {
        return;
    }

    let too_large = layers
        .iter()
        .position(|layer| u64::try_from(layer.len()).expect("a layer's length") > bound);
    // The stray byte follows the outermost stream, so only that layer's own
    // output can pass the bound before it is seen.
    match (too_large, stray) {
        (Some(0), _) | (Some(_), false) => assert!(
            matches!(handed, Err(Reason::TooLarge { .. })),
            "{field:?} handed on {handed:?} past {bound}"
        ),
        (_, true) => assert_eq!(
            handed,
            Err(Reason::Malformed),
            "{field:?} with a stray byte"
        ),
        (None, false) => assert_eq!(handed.as_deref(), Ok(payload), "{field:?}"),
    }
});

/// `octets` under `coding`, by the coding's reference encoder.
fn encoded(coding: Coding, octets: &[u8]) -> Vec<u8> {
    match coding {
        Coding::Gzip => {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            encoder.write_all(octets).expect("an in-memory write");
            encoder.finish().expect("an in-memory write")
        }
        Coding::Brotli => {
            let mut encoded = Vec::new();
            {
                let mut encoder = brotli::CompressorWriter::new(&mut encoded, 4096, 1, 22);
                encoder.write_all(octets).expect("an in-memory write");
            }
            encoded
        }
        Coding::Zstd => zstd::encode_all(octets, 1).expect("an in-memory write"),
    }
}
