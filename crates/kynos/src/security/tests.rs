use std::{cmp::Ordering, collections::HashSet, fmt::Debug, hash::Hash, marker::PhantomData};

use super::{
    constant_time_eq,
    requirement::{AllOf, AnyOf},
    schemes::{Basic, Bearer, MutualTls},
};

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

/// What a verified credential yields, deriving nothing, as an application's
/// own claims type may.
struct Claims;

/// A scope deriving nothing.
struct Grant;

/// A realm deriving nothing.
struct Admin;

/// Asserts the traits every scheme marker and combinator carries hold on `M`
/// whatever its parameters derive, and that its one value is equal to,
/// ordered as and hashed as itself, and prints as `debug`.
fn holds_marker_traits<M>(debug: &str)
where
    M: Clone + Copy + Debug + Default + Eq + Ord + Hash,
{
    let marker = M::default();
    let copied = marker;

    assert_eq!(marker.clone(), copied);
    assert_eq!(marker.partial_cmp(&copied), Some(Ordering::Equal));
    assert_eq!(marker.cmp(&copied), Ordering::Equal);
    assert_eq!(
        HashSet::from([marker, copied]).len(),
        1,
        "equal markers hash alike"
    );
    assert_eq!(format!("{marker:?}"), debug);
}

/// The claims and scope sit only in a `PhantomData`, so no trait on `Bearer`
/// is bounded on them.
#[test]
fn bearer_holds_its_traits_over_claims_and_scope_deriving_nothing() {
    holds_marker_traits::<Bearer<Claims, Grant>>(&format!(
        "Bearer({:?})",
        PhantomData::<fn() -> (Claims, Grant)>
    ));
}

/// As for `Bearer`, and the realm is held the same way.
#[test]
fn basic_holds_its_traits_over_claims_scope_and_realm_deriving_nothing() {
    holds_marker_traits::<Basic<Claims, Grant, Admin>>(&format!(
        "Basic({:?})",
        PhantomData::<fn() -> (Claims, Grant, Admin)>
    ));
}

/// As for `Bearer`.
#[test]
fn mutual_tls_holds_its_traits_over_claims_and_scope_deriving_nothing() {
    holds_marker_traits::<MutualTls<Claims, Grant>>(&format!(
        "MutualTls({:?})",
        PhantomData::<fn() -> (Claims, Grant)>
    ));
}

/// The schemes are held only in a `PhantomData`, so `AnyOf` asks nothing of
/// them, nor of their claims and scopes.
#[test]
fn any_of_holds_its_traits_over_schemes_whose_claims_and_scope_derive_nothing() {
    type Schemes = (Bearer<Claims, Grant>, MutualTls<Claims, Grant>);

    holds_marker_traits::<AnyOf<Schemes>>(&format!("AnyOf({:?})", PhantomData::<fn() -> Schemes>));
}

/// As for `AnyOf`.
#[test]
fn all_of_holds_its_traits_over_schemes_whose_claims_and_scope_derive_nothing() {
    type Schemes = (Bearer<Claims, Grant>, MutualTls<Claims, Grant>);

    holds_marker_traits::<AllOf<Schemes>>(&format!("AllOf({:?})", PhantomData::<fn() -> Schemes>));
}
