//! The `Accept-Encoding` reader `Compression` and the embedded assets choose a
//! coding with, in `http/coding.rs`.
//!
//! The input is one field value. Every weight read is a qvalue, at most 1000,
//! and reads the same from the field in any ASCII case, since coding names and
//! the `q` parameter are case-insensitive (RFC 9110 sections 8.4.1 and 12.4.2).
//!
//! The coding chosen is the one RFC 9110 section 12.5.3 has the client prefer:
//! acceptable, weighted at least as identity is, outranked by no coding offered,
//! and tied by none offered before it. Where none is chosen, no coding offered
//! is acceptable at a weight identity does not beat.

#![no_main]

use kynos::__private::fuzz::{coding_quality, identity_quality, preferred_coding};
use libfuzzer_sys::fuzz_target;

/// The codings offered, in the order a tie between them is broken by.
const OFFERED: [&str; 4] = ["zstd", "br", "gzip", "deflate"];

fuzz_target!(|data: &[u8]| {
    let Ok(accept) = std::str::from_utf8(data) else {
        return;
    };
    let shouted = accept.to_ascii_uppercase();

    let identity = identity_quality(accept);
    assert!(identity <= 1_000, "identity at {identity} in {accept:?}");
    assert_eq!(identity_quality(&shouted), identity, "{accept:?}");

    let weights = OFFERED.map(|token| {
        let weight = coding_quality(accept, token);
        assert!(
            weight.is_none_or(|weight| weight <= 1_000),
            "{token} at {weight:?} in {accept:?}"
        );
        assert_eq!(
            coding_quality(&shouted, token),
            weight,
            "{token} in {accept:?}"
        );
        weight.unwrap_or(0)
    });

    match preferred_coding(accept, &OFFERED) {
        Some(chosen) => {
            let at = OFFERED
                .iter()
                .position(|token| *token == chosen)
                .expect("a coding offered");
            let weight = weights[at];
            assert!(weight > 0 && weight >= identity, "{chosen} in {accept:?}");
            for (index, other) in weights.iter().enumerate() {
                if index < at {
                    assert!(
                        *other < weight,
                        "{} before {chosen} in {accept:?}",
                        OFFERED[index]
                    );
                } else {
                    assert!(
                        *other <= weight,
                        "{} over {chosen} in {accept:?}",
                        OFFERED[index]
                    );
                }
            }
        }
        None => {
            for (token, weight) in OFFERED.iter().zip(weights) {
                assert!(
                    weight == 0 || weight < identity,
                    "{token} at {weight} passed over in {accept:?}"
                );
            }
        }
    }
});
