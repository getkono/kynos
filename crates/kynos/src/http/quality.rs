//! Reading the `q` parameter RFC 9110 section 12.4.2 defines.
//!
//! Shared by [`negotiate`](crate::response::negotiate),
//! [`language`](crate::response::language) and
//! [`coding::quality`](super::coding::quality); each decides what a refusal
//! means.

/// The weight `value` states, in thousandths.
///
/// `None` when it is not a qvalue: `1.5` and `0.1234` are refused, not rounded.
pub(crate) fn parse(value: &str) -> Option<u16> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if fraction.len() > 3 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    match whole {
        // `"0" [ "." 0*3DIGIT ]`, so `0.` is as much a zero as `0.000`.
        "0" => Some(
            fraction
                .bytes()
                .zip([100, 10, 1])
                .map(|(digit, place)| u16::from(digit - b'0') * place)
                .sum(),
        ),
        // `"1" [ "." 0*3("0") ]`: 1 admits only zeros after the point.
        "1" => fraction.bytes().all(|byte| byte == b'0').then_some(1_000),
        _ => None,
    }
}

/// The weight a list element's `parameters` state, in thousandths: the last
/// `q`'s, or `1` where none is `q`.
///
/// `None` when a parameter has no `=` or a `q` is not a qvalue.
pub(crate) fn weight<'a>(parameters: impl IntoIterator<Item = &'a str>) -> Option<u16> {
    let mut quality = 1_000;
    for parameter in parameters {
        let (name, value) = parameter.trim().split_once('=')?;
        if name.trim().eq_ignore_ascii_case("q") {
            quality = parse(value.trim())?;
        }
    }
    Some(quality)
}

#[cfg(test)]
mod tests;
