use super::decode;

/// An encoder written from the specification rather than from the decoder.
///
/// `docs/testing.md`'s parser rule turns on this being *independent*: an
/// oracle derived from the code under test agrees with it by construction,
/// including wherever both are wrong. This one is a transcription of RFC
/// 4648 section 4 and never consults `decode`.
fn encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::new();
    for chunk in input.chunks(3) {
        let mut group = [0u8; 3];
        group[..chunk.len()].copy_from_slice(chunk);
        let packed = (u32::from(group[0]) << 16) | (u32::from(group[1]) << 8) | u32::from(group[2]);

        for index in 0..4 {
            if index <= chunk.len() {
                let sextet = (packed >> (18 - index * 6)) & 0x3f;
                out.push(char::from(ALPHABET[sextet as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The vectors RFC 4648 section 10 publishes, which fix every padding case.
#[test]
fn the_published_vectors_decode_to_what_they_name() {
    let vectors = [
        ("", ""),
        ("Zg==", "f"),
        ("Zm8=", "fo"),
        ("Zm9v", "foo"),
        ("Zm9vYg==", "foob"),
        ("Zm9vYmE=", "fooba"),
        ("Zm9vYmFy", "foobar"),
    ];

    for (encoded, expected) in vectors {
        assert_eq!(
            decode(encoded).as_deref(),
            Some(expected.as_bytes()),
            "decoding {encoded:?}"
        );
    }
}

/// `decode ∘ encode` is the identity, over every length that reaches each
/// padding case and every byte value.
///
/// A sweep rather than a draw: the space that matters is the residue of the
/// length modulo three, and closing it is stronger than sampling it.
#[test]
fn every_input_survives_its_own_encoding() {
    for length in 0..=64usize {
        // A spread of byte values rather than a run, so a packing that
        // dropped a sextet would show up as a different byte.
        let input: Vec<u8> = (0..length)
            .map(|index| u8::try_from(index * 7 % 256).expect("a byte"))
            .collect();
        let encoded = encode(&input);
        assert_eq!(
            decode(&encoded),
            Some(input.clone()),
            "round trip of {length} byte(s) through {encoded:?}"
        );
    }

    // Every byte value, so no branch of the packing is missed.
    let every: Vec<u8> = (0..=255u8).collect();
    assert_eq!(decode(&encode(&every)), Some(every));
}

/// One case per way `decode` refuses, counted against the refusals.
#[test]
fn every_refusal_has_a_case() {
    const SOURCE: &str = include_str!("../base64.rs");
    // Spelled in two pieces, since `SOURCE` is this file and a contiguous
    // literal would count itself -- the idiom `schemes.rs` already uses.
    const NEEDLE: &str = concat!("return ", "None;");

    let cases = [
        ("a length that is not a multiple of four", "Zm9"),
        ("a character outside the alphabet", "Zm9-"),
        ("padding before the end", "Z=9v"),
        ("more padding than a group can carry", "Zg=="), // legal; see below
        ("bits set past the end of the last byte", "Zh=="),
    ];

    // The fourth row is the control: two padding characters are legal, and
    // without it "more than two" would read as "two or more".
    assert!(decode(cases[3].1).is_some(), "{}", cases[3].0);
    assert!(decode("Zm9vYg====").is_none(), "four padding characters");

    for (description, input) in [cases[0], cases[1], cases[2], cases[4]] {
        assert!(decode(input).is_none(), "{description}: {input:?}");
    }

    // The refusals in the body, plus the `?` that rejects a character
    // outside the alphabet, which returns without saying so.
    let refusals = SOURCE.matches(NEEDLE).count() + 1;
    assert_eq!(
        refusals, 5,
        "`base64.rs` refuses {refusals} way(s) and five have a case; a refusal added without \
         one is a way of spelling a credential that nothing checks"
    );
}

/// Whitespace is not a separator here, whatever other decoders allow.
#[test]
fn a_token_carrying_whitespace_is_refused() {
    assert!(decode("Zm9v YmFy").is_none());
    assert!(decode("Zm9v\nYmFy").is_none());
}
