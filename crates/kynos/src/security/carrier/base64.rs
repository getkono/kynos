//! Decoding base64, for the one credential whose wire form needs it.
//!
//! Private, and decode-only. HTTP basic authentication is the only thing in
//! Kynos that meets base64, it only ever reads, and RFC 7617 fixes exactly one
//! alphabet — so what a dependency would buy here is an encoder nothing calls,
//! a URL-safe alphabet nothing sends, and a streaming interface for a string
//! that is already in memory.
//!
//! `docs/architecture.md` records the refusal: a new dependency arrives
//! feature-gated and additive, and basic authentication is in the default
//! build, so `base64` could not have been gated.

/// The value of one alphabet character, per RFC 4648 section 4.
const fn sextet(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decodes standard, padded base64.
///
/// `None` for anything that is not one: a length that is not a multiple of
/// four, a character outside the alphabet, padding anywhere but the end, more
/// than two padding characters, or bits set past the end of the last byte.
///
/// Strict on every count, deliberately. A lenient decoder accepts several
/// encodings of one credential, and a credential with more than one spelling is
/// one an allow-list can be walked past. Whitespace is refused for the same
/// reason: RFC 7617 does not permit it inside the token.
pub(super) fn decode(input: &str) -> Option<Vec<u8>> {
    let bytes = input.as_bytes();
    if bytes.is_empty() {
        return Some(Vec::new());
    }
    if bytes.len() % 4 != 0 {
        return None;
    }

    // Padding is legal only as the last one or two characters.
    let padding = bytes.iter().rev().take_while(|&&byte| byte == b'=').count();
    if padding > 2 {
        return None;
    }
    let body = &bytes[..bytes.len() - padding];
    if body.contains(&b'=') {
        return None;
    }

    let mut decoded = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let mut accumulated: u32 = 0;
        let mut held = 0;
        for &byte in chunk {
            if byte == b'=' {
                break;
            }
            accumulated = (accumulated << 6) | u32::from(sextet(byte)?);
            held += 6;
        }

        // The bits a partial group leaves over belong to no byte, and RFC 4648
        // section 3.5 requires them to be zero. A decoder that ignored them
        // would give one credential several spellings.
        let whole = held / 8;
        let leftover = held % 8;
        if leftover != 0 && accumulated & ((1 << leftover) - 1) != 0 {
            return None;
        }
        accumulated >>= leftover;

        for index in (0..whole).rev() {
            decoded.push(((accumulated >> (index * 8)) & 0xff) as u8);
        }
    }

    Some(decoded)
}

#[cfg(test)]
mod tests {
    use super::decode;

    /// An encoder written from the specification rather than from the decoder.
    ///
    /// `docs/testing.md`'s parser rule turns on this being *independent*: an
    /// oracle derived from the code under test agrees with it by construction,
    /// including wherever both are wrong. This one is a transcription of RFC
    /// 4648 section 4 and never consults `decode`.
    fn encode(input: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

        let mut out = String::new();
        for chunk in input.chunks(3) {
            let mut group = [0u8; 3];
            group[..chunk.len()].copy_from_slice(chunk);
            let packed =
                (u32::from(group[0]) << 16) | (u32::from(group[1]) << 8) | u32::from(group[2]);

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
        const SOURCE: &str = include_str!("base64.rs");
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
}
