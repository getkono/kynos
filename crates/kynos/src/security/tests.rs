use super::constant_time_eq;

/// Agreement with `==`, swept over every way two short strings can differ.
///
/// The property is that this is `==` and nothing else: a comparison that
/// were merely *slow* would satisfy any single case. What cannot be
/// asserted here is the timing itself — a wall-clock assertion is a flake
/// on a shared runner, and `docs/testing.md` would rather have no test than
/// a retried one — so the constant-time half is documented as best-effort
/// on the item instead.
#[test]
fn the_comparison_agrees_with_equality_everywhere() {
    let inputs: &[&[u8]] = &[
        b"",
        b"a",
        b"b",
        b"ab",
        b"ba",
        b"aa",
        b"abc",
        b"abd",
        b"dbc",
        b"abcd",
        &[0x00],
        &[0xff],
        &[0x00, 0x00],
    ];

    for left in inputs {
        for right in inputs {
            assert_eq!(
                constant_time_eq(left, right),
                left == right,
                "comparing {left:?} against {right:?}"
            );
        }
    }
}

/// A difference in the last byte is found as surely as one in the first.
///
/// The failure this rules out is a fold that stops early: with `&&` in
/// place of `|`, a secret differing only at the end would compare equal for
/// every prefix that matched.
#[test]
fn a_difference_anywhere_is_a_difference() {
    let secret = b"0123456789abcdef";

    for index in 0..secret.len() {
        let mut guess = *secret;
        guess[index] ^= 0x01;
        assert!(
            !constant_time_eq(secret, &guess),
            "a difference at byte {index} went unnoticed"
        );
    }

    assert!(constant_time_eq(secret, secret));
}
