//! The record framer behind `JsonLines` and `JsonSeq`.
//!
//! The input's first byte picks the framing, newline-terminated NDJSON or RFC
//! 7464's separator-prefixed sequence; the next two the body limit; the next
//! how many chunk lengths follow, each one byte; the rest is the body, split
//! into chunks of those lengths and a last one holding what remains.
//!
//! However the body is split, it reads as the whole buffer framed at once
//! reads: every delimited record in order, a blank one skipped, each decoded
//! record a JSON value or a 422 under its index that reading continues past,
//! and the stream ending on a record that is not JSON (400), on a record
//! longer than the limit (413), or, for a sequence, on a body that does not
//! open with a separator (400).

#![no_main]

use kynos::{__private::fuzz, error::rejection::BodyRejection};
use libfuzzer_sys::fuzz_target;

/// RFC 7464's record separator.
const RECORD_SEPARATOR: u8 = 0x1e;

/// The most chunk lengths an input states.
const MAX_CUTS: u8 = 16;

/// What reading one record should produce.
#[derive(Debug, PartialEq)]
enum Expected {
    /// The record's JSON value.
    Value(serde_json::Value),
    /// A 422 keyed by the record's index; reading goes on.
    Schema(String),
    /// A 400, which ends the stream.
    Syntax,
    /// A 413 naming the limit, which ends the stream.
    TooLarge(u64),
}

fuzz_target!(|data: &[u8]| {
    let [flags, high, low, cuts, rest @ ..] = data else {
        return;
    };
    let sequence = flags & 1 != 0;
    let limit = u64::from(u16::from_be_bytes([*high, *low]));
    let cuts = usize::from(cuts % (MAX_CUTS + 1)).min(rest.len());
    let (lengths, body) = rest.split_at(cuts);

    let mut chunks = Vec::new();
    let mut remaining = body;
    for &length in lengths {
        let (chunk, after) = remaining.split_at(usize::from(length).min(remaining.len()));
        chunks.push(chunk);
        remaining = after;
    }
    chunks.push(remaining);

    let read: Vec<Expected> = fuzz::json_records(sequence, &chunks, limit)
        .into_iter()
        .map(|record| match record {
            Ok(value) => Expected::Value(value),
            Err(BodyRejection::Schema { failures }) => {
                let pointers: Vec<String> = failures.into_keys().collect();
                assert_eq!(pointers.len(), 1, "one pointer per record: {pointers:?}");
                Expected::Schema(pointers.into_iter().next().unwrap_or_default())
            }
            Err(BodyRejection::Syntax { .. }) => Expected::Syntax,
            Err(BodyRejection::TooLarge { limit }) => Expected::TooLarge(limit),
            Err(other) => panic!("a framing failure is no {other:?}"),
        })
        .collect();

    assert_eq!(read, whole(sequence, body, limit), "{chunks:?}");
});

/// What `body`, framed as one buffer, reads as.
fn whole(sequence: bool, body: &[u8], limit: u64) -> Vec<Expected> {
    // Each record as delimited, before any whitespace is trimmed.
    let delimited: Vec<&[u8]> = if sequence {
        match body
            .split(|&byte| byte == RECORD_SEPARATOR)
            .collect::<Vec<_>>()[..]
        {
            // Nothing at all, or nothing before the first separator.
            [[], ref records @ ..] => records.to_vec(),
            _ => return vec![Expected::Syntax],
        }
    } else {
        body.split(|&byte| byte == b'\n').collect()
    };

    let mut read = Vec::new();
    let mut index = 0;
    for record in delimited {
        if u64::try_from(record.len()).expect("a record's length") > limit {
            read.push(Expected::TooLarge(limit));
            break;
        }
        let record = record.trim_ascii();
        if record.is_empty() {
            continue;
        }
        match serde_json::from_slice(record) {
            Ok(value) => read.push(Expected::Value(value)),
            Err(error) if error.is_data() => read.push(Expected::Schema(format!("/{index}"))),
            Err(_) => {
                read.push(Expected::Syntax);
                break;
            }
        }
        index += 1;
    }
    read
}
