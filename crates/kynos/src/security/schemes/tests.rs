use kynos_openapi::SecurityScheme as Described;

use super::{Basic, Bearer, Credentials, MutualTls};
use crate::security::SecurityScheme;

/// One row per scheme, over the whole set.
///
/// A closed enumeration is checked across all of it because a sample of one
/// reads as the whole and is not: `docs/testing.md` records
/// `the_style_location_table_is_closed` asserting five of forty pairs and
/// the name claiming closedness that the body did not have.
fn row<S: SecurityScheme>() -> (&'static str, Described, Option<&'static str>) {
    (S::NAME, S::describe(), S::challenge())
}

#[test]
fn every_configuration_free_scheme_describes_itself_and_names_its_challenge() {
    let table = [
        row::<Bearer>(),
        row::<Basic<Credentials>>(),
        row::<MutualTls>(),
    ];

    assert_eq!(
        table,
        [
            ("Bearer", Described::bearer(None), Some("Bearer")),
            // RFC 7617 section 2: the `charset` parameter is what tells a
            // client to send a non-ASCII password as UTF-8, and `UTF-8` is
            // the only value the registry defines. A bare `Basic` leaves
            // every client to guess, and they do not all guess the same.
            (
                "Basic",
                Described::basic(),
                Some(r#"Basic charset="UTF-8""#)
            ),
            // No challenge: the certificate is presented during the TLS
            // handshake, so a 401 has no `WWW-Authenticate` scheme to name
            // -- there is none registered for it, and inventing one would
            // advertise a challenge no client could answer.
            ("MutualTls", Described::mutual_tls(), None),
        ]
    );
}

/// The set, counted against the rows above.
///
/// Witnessing three says nothing about whether three is all of them. This
/// reads the count out of the source, so a fourth scheme added without a
/// row fails the build.
#[test]
fn every_configuration_free_scheme_has_a_row() {
    const SOURCE: &str = include_str!("../schemes.rs");
    /// `Bearer`, `Basic` and `MutualTls`.
    const WITNESSED: usize = 3;

    // Spelled in two pieces: `SOURCE` is this file, so a contiguous
    // literal would count itself.
    const NEEDLE: &str = concat!("> SecurityScheme", " for ");

    let declared = SOURCE.matches(NEEDLE).count();

    assert_eq!(
        declared, WITNESSED,
        "`schemes.rs` implements `SecurityScheme` {declared} time(s) and {WITNESSED} have a \
         row; a scheme added without one is a scheme nothing describes"
    );
}

/// The credential type is the application's, and the description is the
/// same whatever it is.
///
/// Without this, `Bearer<T>`'s parameter could be dropped and nothing would
/// notice — and an application could then only ever have one bearer
/// authenticator, handing every handler a raw `String`.
#[test]
fn the_credential_type_does_not_reach_the_description() {
    struct Claims;

    assert_eq!(
        <Bearer<Claims> as SecurityScheme>::describe(),
        <Bearer<String> as SecurityScheme>::describe()
    );
    assert_eq!(
        <Bearer<Claims> as SecurityScheme>::NAME,
        <Bearer<String> as SecurityScheme>::NAME
    );
}
