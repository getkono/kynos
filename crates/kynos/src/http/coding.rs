//! Reading `Accept-Encoding`.
//!
//! Shared by [`Compression`](crate::middleware::compression) and
//! [`assets`](crate::router::assets) so both read RFC 9110 section 12.5.3 alike.

/// The deprecated spellings a recipient must treat as `token`.
///
/// RFC 9110 sections 8.4.1.1 and 8.4.1.3; only `gzip` has one among the
/// codings Kynos names.
fn aliases(token: &str) -> &'static [&'static str] {
    match token {
        "gzip" => &["x-gzip"],
        _ => &[],
    }
}

/// The quality `accept` assigns `token`, honouring `*`, in thousandths.
///
/// `0..=1000` (RFC 9110 section 12.4.2). `None` when neither the token nor a
/// wildcard appears, as distinct from `Some(0)`, refused.
///
/// Whether identity is acceptable is [`identity_quality`]'s question, which
/// reads a malformed or absent weight differently.
#[must_use]
pub(crate) fn quality(accept: &str, token: &str) -> Option<u16> {
    // A malformed weight (including `q=1.5`) refuses rather than outranking
    // a legitimate `q=1`.
    weight(accept, token).map(|weight| match weight {
        Weight::Qvalue(thousandths) => thousandths,
        Weight::Malformed => 0,
    })
}

/// The weight an entry states.
enum Weight {
    /// A qvalue, in thousandths.
    Qvalue(u16),
    /// Not a qvalue; each caller reads it by its own rule.
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
/// `None` means send identity; whether identity was itself refused is
/// [`identity_quality`]'s to say.
///
/// A tie with identity goes to the encoded coding; among encoded codings, to
/// the earlier entry in `available`.
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
/// RFC 9110 section 12.5.3 rule 2: acceptable unless `identity;q=0` or `*;q=0`
/// excludes it. A malformed weight states neither, so leaves the default 1000.
#[must_use]
pub(crate) fn identity_quality(accept: &str) -> u16 {
    match weight(accept, "identity") {
        Some(Weight::Qvalue(thousandths)) => thousandths,
        None | Some(Weight::Malformed) => 1_000,
    }
}

#[cfg(test)]
mod tests;
