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
mod tests;
