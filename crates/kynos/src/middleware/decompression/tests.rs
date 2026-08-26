//! The parts of decompression that decide before anything is read.

use super::{Coding, Decompression, MAX_CODINGS, declared};
use crate::http;

/// Builds a header map carrying `values` as `Content-Encoding`, one field line
/// each -- which is a shape a client may legitimately send, and which a parser
/// reading only the first value would miss.
fn encoded(values: &[&str]) -> http::HeaderMap {
    let mut headers = http::HeaderMap::new();

    for value in values {
        headers.append(
            http::header::CONTENT_ENCODING,
            http::HeaderValue::from_str(value).expect("a usable field value"),
        );
    }

    headers
}

/// The token table, swept rather than sampled. A coding added without a
/// spelling here is a coding this fails on.
#[test]
fn every_coding_is_named_by_the_token_it_is_named_by() {
    const CASES: &[(&str, Option<Coding>)] = &[
        ("zstd", Some(Coding::Zstd)),
        ("br", Some(Coding::Brotli)),
        ("gzip", Some(Coding::Gzip)),
        // RFC 9110 section 8.4.1.3 keeps `x-gzip` as an alias, and clients
        // still send it.
        ("x-gzip", Some(Coding::Gzip)),
        // Section 8.4.1 makes content codings case-insensitive, so a client
        // shouting is a client to be understood rather than refused.
        ("GZIP", Some(Coding::Gzip)),
        ("Br", Some(Coding::Brotli)),
        ("ZStD", Some(Coding::Zstd)),
        // Registered codings this crate does not implement, and one that is
        // not a coding at all. Each must be refused rather than ignored: a body
        // handed on undecoded is a body the handler reads as garbage.
        ("deflate", None),
        ("compress", None),
        ("x-compress", None),
        ("snappy", None),
        ("", None),
    ];

    for (token, expected) in CASES {
        assert_eq!(
            Coding::from_token(token),
            *expected,
            "the token {token:?} was not read as {expected:?}"
        );
    }
}

#[test]
fn a_body_that_names_no_coding_names_no_coding() {
    assert_eq!(declared(&http::HeaderMap::new()), Some(Vec::new()));
}

/// RFC 9110 section 8.4: `identity` SHOULD NOT appear, and a sender that
/// includes it anyway means the body was not encoded. Refusing it would refuse
/// a body that is perfectly readable.
#[test]
fn identity_is_dropped_rather_than_refused() {
    assert_eq!(declared(&encoded(&["identity"])), Some(Vec::new()));
    assert_eq!(
        declared(&encoded(&["identity, gzip"])),
        Some(vec![Coding::Gzip])
    );
}

/// Section 8.4 lists the codings in the order they were applied, so the list is
/// read in that order and undone in reverse.
#[test]
fn a_chain_is_read_in_the_order_it_was_applied() {
    assert_eq!(
        declared(&encoded(&["gzip, br"])),
        Some(vec![Coding::Gzip, Coding::Brotli])
    );
}

/// `Content-Encoding` is a list header, and a list header may arrive split
/// across field lines. Reading only the first would decode half a chain and
/// hand the rest on as garbage.
#[test]
fn a_chain_split_across_field_lines_is_read_whole() {
    assert_eq!(
        declared(&encoded(&["gzip", "br"])),
        Some(vec![Coding::Gzip, Coding::Brotli])
    );
}

#[test]
fn an_unknown_coding_refuses_the_whole_body() {
    assert_eq!(declared(&encoded(&["deflate"])), None);
    assert_eq!(
        declared(&encoded(&["gzip, deflate"])),
        None,
        "a chain is only decodable if every link is"
    );
}

/// Each link costs a decode pass over a body already at the cap, so a long
/// chain is a way to buy work with a small request.
#[test]
fn a_chain_longer_than_the_cap_is_refused() {
    let longest = ["gzip"; MAX_CODINGS].join(", ");
    let overlong = ["gzip"; MAX_CODINGS + 1].join(", ");

    assert!(
        declared(&encoded(&[&longest])).is_some(),
        "the longest permitted chain was refused"
    );
    assert_eq!(declared(&encoded(&[&overlong])), None);
}

/// Unset, the absolute limit is the only bound -- so a body is never refused
/// for expanding, only for being large.
#[test]
fn without_a_ratio_the_absolute_limit_is_the_only_bound() {
    let decompression = Decompression::new(1_000);

    assert_eq!(decompression.bound(1), 1_000);
    assert_eq!(decompression.bound(u64::MAX), 1_000);
}

/// The ratio is the cheaper check and must be the one that binds while it is
/// tighter, or a small body could reach the absolute cap unchallenged.
#[test]
fn the_bound_is_whichever_limit_is_tighter() {
    let decompression = Decompression::new(1_000).max_ratio(10);

    assert_eq!(decompression.bound(1), 10, "the ratio should have bound it");
    assert_eq!(
        decompression.bound(1_000),
        1_000,
        "the absolute limit should have bound it"
    );
    assert_eq!(
        decompression.bound(100),
        1_000,
        "the two coincide here, and the answer is the shared value"
    );
}

/// A body large enough that its ratio bound would overflow must still be bound
/// by the absolute limit rather than wrapping to something small -- or large.
#[test]
fn a_ratio_that_would_overflow_falls_back_to_the_absolute_limit() {
    let decompression = Decompression::new(1_000).max_ratio(u64::MAX);

    assert_eq!(decompression.bound(u64::MAX), 1_000);
}
