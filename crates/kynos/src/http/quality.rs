//! Reading the `q` parameter RFC 9110 section 12.4.2 defines.
//!
//! Private, and `pub(crate)`, because a weight is not a type an application
//! names — it reaches a handler already folded into whichever alternative won.
//! It lives here rather than beside any caller because three negotiated axes
//! read it: [`negotiate`](crate::response::negotiate) ranks media types,
//! [`language`](crate::response::language) ranks language ranges and
//! [`coding::quality`](super::coding::quality) weighs content codings, and
//! section 12.4.2 is one grammar shared by every `Accept*` field rather than
//! one per field. What a refusal means is each caller's: the first two reject
//! the field, where a content coding weight that is not a qvalue refuses that
//! coding but excludes no identity, which only an explicit `q=0` does.

/// The weight `value` states, in thousandths.
///
/// `None` when it is not a qvalue. Section 12.4.2 bounds one at three decimal
/// places and at 1, so `1.5` and `0.1234` are both refusals rather than values
/// to round — a field that says something the grammar cannot express is one
/// this parser declines to guess at.
pub(crate) fn parse(value: &str) -> Option<u16> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if fraction.len() > 3 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    match whole {
        // `"0" [ "." 0*3DIGIT ]`: zero to three digits, each a place of
        // thousandths, so `0.` is as much a zero as `0.000`.
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

#[cfg(test)]
mod tests;
