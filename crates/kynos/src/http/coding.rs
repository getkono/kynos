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

/// The quality `accept` assigns `token`, honouring `*`, in thousandths.
///
/// A weight is `0..=1000`: RFC 9110 section 12.4.2 bounds a qvalue at three
/// decimal places, so thousandths state every one exactly. A weight the
/// grammar cannot express is `0`, a refusal. `None` when neither the token nor
/// a wildcard appears, which is what distinguishes "not mentioned" from
/// "mentioned and refused" — the difference between the two is the whole of
/// `q=0`.
///
/// This reads the weight the field gives a coding, `identity` included; it does
/// not say whether identity is acceptable, which is RFC 9110 section 12.5.3
/// rule 2's question and [`identity_quality`]'s. The two differ where a weight
/// is not a qvalue: `quality("identity;q=1.5", "identity")` and
/// `quality("*;q=1.5", "identity")` are `Some(0)`, yet neither field states the
/// `identity;q=0` or `*;q=0` that excludes identity, so `identity_quality`
/// reads both as 1000. They differ where the field is silent too: `None` here,
/// 1000 there.
#[must_use]
pub(crate) fn quality(accept: &str, token: &str) -> Option<u16> {
    // A malformed weight is a refusal rather than a default: a client that
    // wrote something RFC 9110 section 12.4.2 cannot express did not ask for
    // this coding. That includes a value above 1, which read literally would
    // let `gzip;q=1.5` outrank a legitimate `q=1`.
    weight(accept, token).map(|weight| match weight {
        Weight::Qvalue(thousandths) => thousandths,
        Weight::Malformed => 0,
    })
}

/// The weight an entry states.
enum Weight {
    /// A qvalue, in thousandths.
    Qvalue(u16),
    /// Something RFC 9110 section 12.4.2 cannot express, which each caller
    /// reads by its own rule.
    Malformed,
}

/// The weight of the entry that speaks for `token`: its own, else the
/// wildcard's. `None` when neither appears.
fn weight(accept: &str, token: &str) -> Option<Weight> {
    let mut wildcard = None;

    for entry in accept.split(',') {
        let mut parts = entry.split(';');
        let name = parts.next().unwrap_or_default().trim();

        let weight = parts
            .find_map(|parameter| {
                let parameter = parameter.trim();
                parameter
                    .strip_prefix("q=")
                    .or_else(|| parameter.strip_prefix("Q="))
            })
            .map_or(Weight::Qvalue(1_000), |weight| {
                super::quality::parse(weight.trim()).map_or(Weight::Malformed, Weight::Qvalue)
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
#[cfg_attr(
    not(any(test, feature = "assets")),
    expect(dead_code, reason = "the asset server is its only caller")
)]
pub(crate) fn preferred<'a>(accept: &str, available: &[&'a str]) -> Option<&'a str> {
    let mut best: Option<(&'a str, u16)> = None;

    for token in available {
        let Some(weight) = quality(accept, token) else {
            continue;
        };
        if weight == 0 {
            continue;
        }
        if best.is_none_or(|(_, best)| weight > best) {
            best = Some((token, weight));
        }
    }

    let (token, weight) = best?;
    (identity_quality(accept) <= weight).then_some(token)
}

/// What the client thinks of the unencoded representation, in thousandths.
///
/// RFC 9110 section 12.5.3 rule 2: identity "is acceptable by default unless
/// specifically excluded by the Accept-Encoding header field stating either
/// `identity;q=0` or `*;q=0` without a more specific entry for `identity`".
/// Both spellings read as `0`. A weight that is not a qvalue, such as
/// `identity;q=1.5` or `*;q=1.5`, states neither, so it leaves identity at its
/// default of 1000 — even though the same wildcard refuses every coding it
/// speaks for.
#[must_use]
pub(crate) fn identity_quality(accept: &str) -> u16 {
    match weight(accept, "identity") {
        Some(Weight::Qvalue(thousandths)) => thousandths,
        None | Some(Weight::Malformed) => 1_000,
    }
}

#[cfg(test)]
mod tests;
