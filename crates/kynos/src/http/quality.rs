//! Reading the `q` parameter RFC 9110 section 12.4.2 defines.
//!
//! Private, and `pub(crate)`, because a weight is not a type an application
//! names — it reaches a handler already folded into whichever alternative won.
//! It lives here rather than beside either caller because two negotiated axes
//! read it: [`negotiate`](crate::response::negotiate) ranks media types and
//! [`language`](crate::response::language) ranks language ranges, and section
//! 12.4.2 is one grammar shared by every `Accept*` field rather than one per
//! field.
//!
//! [`coding::quality`](super::coding::quality) deliberately does not call this,
//! and the difference is real rather than accidental: it reads `q=1.5` as `1.0`
//! on the argument that the client did ask for the coding, where this refuses
//! the field outright. Unifying them is a behaviour change to one caller or the
//! other and belongs in its own commit.

/// The weight `value` states, in thousandths.
///
/// `None` when it is not a qvalue. Section 12.4.2 bounds one at three decimal
/// places and at 1, so `1.5` and `0.1234` are both refusals rather than values
/// to round — a field that says something the grammar cannot express is one
/// this parser declines to guess at.
pub(crate) fn parse(value: &str) -> Option<u16> {
    if value == "0" || value == "0.0" || value == "0.00" || value == "0.000" {
        return Some(0);
    }
    if value == "1" || value == "1.0" || value == "1.00" || value == "1.000" {
        return Some(1_000);
    }
    let digits = value.strip_prefix("0.")?;
    if digits.is_empty() || digits.len() > 3 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits
        .parse::<u16>()
        .ok()
        .map(|quality| match digits.len() {
            1 => quality * 100,
            2 => quality * 10,
            _ => quality,
        })
}

#[cfg(test)]
mod tests {
    use super::parse;

    /// Every qvalue the grammar admits, against an oracle that never consults
    /// the parser.
    ///
    /// The space closes: section 12.4.2 allows `0`, `1`, and either with one to
    /// three decimal places, which is 1,116 strings. A sweep is total where a
    /// draw from the same space samples it, so this enumerates rather than
    /// generates. The oracle scales a float, which is a different computation
    /// from the digit-shifting under test rather than a transcription of it.
    #[test]
    fn every_qvalue_the_grammar_admits_is_read_as_its_thousandths() {
        let mut swept = 0;

        for whole in ['0', '1'] {
            for places in 0..=3 {
                let mut bodies = vec![String::new()];
                for _ in 0..places {
                    bodies = bodies
                        .iter()
                        .flat_map(|body| ('0'..='9').map(move |digit| format!("{body}{digit}")))
                        .collect();
                }

                for body in bodies {
                    let value = if places == 0 {
                        whole.to_string()
                    } else {
                        format!("{whole}.{body}")
                    };

                    let scaled = value.parse::<f64>().expect("a decimal") * 1000.0;
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "the sweep's own inputs are bounded by 1500"
                    )]
                    let oracle = (scaled.round() as u32 <= 1000).then(|| scaled.round() as u16);

                    assert_eq!(parse(&value), oracle, "q={value}");
                    swept += 1;
                }
            }
        }

        assert_eq!(swept, 2 * (1 + 10 + 100 + 1000), "the space is not closed");
    }

    /// The refusals, one per way the grammar can be missed.
    #[test]
    fn a_weight_the_grammar_cannot_express_is_refused_rather_than_rounded() {
        for value in [
            "1.5",    // above the bound section 12.4.2 sets
            "0.1234", // a fourth decimal place
            "0.",     // a decimal point with no digits
            "0.x",    // a digit that is not one
            "",       // nothing at all
            ".5",     // no whole part
            "2",      // no whole part but 0 and 1
            "-0.5",   // a sign the grammar has no room for
        ] {
            assert_eq!(parse(value), None, "q={value}");
        }
    }
}
