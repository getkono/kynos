use super::parse;

/// Every qvalue the grammar admits, against an oracle that never consults
/// the parser.
///
/// The space closes: `0` or `1`, bare or followed by a decimal point and
/// zero to three digits, is 2,224 candidates. Section 12.4.2 admits 1,117
/// of them, since after `1` it allows only zeros (`0*3("0")`); the oracle
/// refuses the rest. A sweep is total where a draw from the same space
/// samples it, so this enumerates rather than generates. The oracle scales
/// a float, which is a different computation from the digit-shifting under
/// test rather than a transcription of it.
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

            // The whole with a decimal point, then (once) the bare whole:
            // with no digits after it the point alone is still a qvalue.
            let mut values = bodies
                .iter()
                .map(|body| format!("{whole}.{body}"))
                .collect::<Vec<_>>();
            if places == 0 {
                values.push(whole.to_string());
            }

            for value in values {
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

    assert_eq!(
        swept,
        2 * (1 + 1 + 10 + 100 + 1000),
        "the space is not closed"
    );
}

/// The refusals, one per way the grammar can be missed.
#[test]
fn a_weight_the_grammar_cannot_express_is_refused_rather_than_rounded() {
    for value in [
        "1.5",    // above the bound section 12.4.2 sets
        "1.01",   // 1 admits only zeros after the point
        "1.0001", // and at most three of them
        "0.1234", // a fourth decimal place
        "0.x",    // a digit that is not one
        "",       // nothing at all
        ".5",     // no whole part
        "2",      // no whole part but 0 and 1
        "-0.5",   // a sign the grammar has no room for
    ] {
        assert_eq!(parse(value), None, "q={value}");
    }
}
