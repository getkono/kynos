use kynos_openapi::SecurityScheme as Described;

use super::{Basic, Bearer, CHALLENGE_CAPACITY, MutualTls, Realm, compose};
use crate::security::{SecurityScheme, Unscoped, carrier::Credentials};

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
            // RFC 7617 section 2: `realm` is REQUIRED, so an unnamed one is
            // `ApiRealm`'s; and `charset` is what tells a client to send a
            // non-ASCII password as UTF-8, `UTF-8` being the only value the
            // registry defines.
            (
                "Basic",
                Described::basic(),
                Some(r#"Basic realm="api", charset="UTF-8""#)
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

/// A named realm is sent as a quoted-string: `"` and `\` become quoted-pairs
/// (RFC 9110 section 5.6.4), and every other byte a header carries is sent
/// as written, tab and space included.
#[test]
fn a_named_realm_is_sent_as_a_quoted_string() {
    struct Quoted;

    impl Realm for Quoted {
        const REALM: &'static str = "a \"b\"\t\\c~";
    }

    assert_eq!(
        <Basic<Credentials, Unscoped, Quoted> as SecurityScheme>::challenge(),
        Some("Basic realm=\"a \\\"b\\\"\t\\\\c~\", charset=\"UTF-8\"")
    );
}

/// Exactly visible US-ASCII, space and tab are admitted, swept over all of
/// ASCII; the other controls and DEL are refused, and so is anything beyond
/// ASCII, which a field value could hold as `obs-text` but the policy refuses.
#[test]
fn a_realm_admits_exactly_visible_ascii_space_and_tab() {
    for byte in 0..=0x7f_u8 {
        let realm = char::from(byte).to_string();
        let admitted = std::panic::catch_unwind(|| compose(&realm)).is_ok();
        let carried = byte == b'\t' || (b' '..=b'~').contains(&byte);
        assert_eq!(admitted, carried, "{byte:#04x}");
    }

    assert!(std::panic::catch_unwind(|| compose("caf\u{e9}")).is_err());
}

/// The longest realm fits and one byte more is refused, escaping counted.
#[test]
fn a_realm_is_bounded_once_escaped() {
    let longest = "a".repeat(225);
    let (_, len) = compose(&longest);
    assert_eq!(len, CHALLENGE_CAPACITY);

    assert!(std::panic::catch_unwind(|| compose(&"a".repeat(226))).is_err());
    assert!(std::panic::catch_unwind(|| compose(&format!("{}\"", "a".repeat(224)))).is_err());
}
