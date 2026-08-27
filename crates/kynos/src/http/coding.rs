//! Reading `Accept-Encoding`.
//!
//! One place, because two parts of Kynos choose a content coding from the same
//! field and must agree about what it says.
//! [`Compression`](crate::middleware::compression) picks among the codings it
//! can *produce*; [`assets`](crate::router::assets) picks among the codings it
//! has *stored*. The question differs; RFC 9110 section 12.5.3's answer does
//! not, and a second copy of the qvalue rules is a second place they can drift.

/// The deprecated spellings a recipient must treat as `token`.
///
/// RFC 9110 sections 8.4.1.1 and 8.4.1.3: "A recipient SHOULD consider
/// `x-compress` to be equivalent to `compress`" and the same for `x-gzip`.
/// Only `gzip` has one among the codings Kynos names.
fn aliases(token: &str) -> &'static [&'static str] {
    match token {
        "gzip" => &["x-gzip"],
        _ => &[],
    }
}

/// The quality `accept` assigns `token`, honouring `*`.
///
/// `None` when neither the token nor a wildcard appears, which is what
/// distinguishes "not mentioned" from "mentioned and refused" — the difference
/// between the two is the whole of `q=0`.
#[must_use]
pub fn quality(accept: &str, token: &str) -> Option<f32> {
    let mut wildcard = None;

    for entry in accept.split(',') {
        let mut parts = entry.split(';');
        let name = parts.next().unwrap_or_default().trim();

        // A malformed weight is a refusal rather than a default: a client that
        // wrote something unparsable did not ask for this coding.
        let weight = parts
            .find_map(|parameter| {
                let parameter = parameter.trim();
                parameter
                    .strip_prefix("q=")
                    .or_else(|| parameter.strip_prefix("Q="))
            })
            .map_or(1.0, |weight| {
                weight
                    .trim()
                    .parse()
                    // RFC 9110 section 12.4.2 bounds a qvalue at 1. A larger
                    // one is not a qvalue, and reading it literally lets
                    // `gzip;q=1.5` outrank a legitimate `q=1.0` — a preference
                    // inversion a client cannot have meant. Clamped rather than
                    // refused: the client did ask for the coding.
                    .map_or(0.0, |weight: f32| weight.clamp(0.0, 1.0))
            });

        if name.eq_ignore_ascii_case(token)
            || aliases(token)
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(name))
        {
            return Some(weight);
        }

        if name == "*" {
            wildcard = Some(weight);
        }
    }

    wildcard
}

/// The acceptable coding `available` offers that the client prefers most.
///
/// `None` means send the identity representation — either because nothing
/// encoded was acceptable, or because the client preferred identity to
/// everything on offer. A caller that must distinguish "identity is fine" from
/// "identity was refused too" reads [`identity_quality`] as well; the asset
/// server does not, because it always holds the identity octets and a stored
/// representation is never the only one it can send.
///
/// Ties go to the encoded coding, which is what makes a plain
/// `Accept-Encoding: gzip` mean what everybody writes it to mean. Among encoded
/// codings a tie goes to the earlier entry in `available`, so a caller states
/// its own preference by ordering that list.
#[must_use]
pub fn preferred<'a>(accept: &str, available: &[&'a str]) -> Option<&'a str> {
    let mut best: Option<(&'a str, f32)> = None;

    for token in available {
        let Some(weight) = quality(accept, token) else {
            continue;
        };
        if weight <= 0.0 {
            continue;
        }
        if best.is_none_or(|(_, best)| weight > best) {
            best = Some((token, weight));
        }
    }

    let (token, weight) = best?;
    (identity_quality(accept) <= weight).then_some(token)
}

/// What the client thinks of the unencoded representation.
///
/// RFC 9110 section 12.5.3 rule 2: identity "is acceptable by default unless
/// specifically excluded by the Accept-Encoding header field stating either
/// `identity;q=0` or `*;q=0` without a more specific entry for `identity`".
/// [`quality`] falls back to the wildcard, so both spellings land here as
/// `Some(0.0)`.
#[must_use]
pub fn identity_quality(accept: &str) -> f32 {
    quality(accept, "identity").unwrap_or(1.0)
}

#[cfg(test)]
mod tests;
