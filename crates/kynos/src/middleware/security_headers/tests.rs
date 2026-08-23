use std::time::Duration;

use super::{FrameOptions, ReferrerPolicy, SecurityHeaderNames, SecurityHeaders};
use crate::extract::params::header::HeaderParams;

/// What a configuration sends, as name/value pairs.
fn sent(headers: &SecurityHeaders, secure: bool) -> Vec<(String, String)> {
    headers
        .fields(secure)
        .into_iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                value.to_str().expect("a printable field").to_owned(),
            )
        })
        .collect()
}

/// The default set, and nothing beyond it.
///
/// CSP and HSTS are deployment decisions rather than safe defaults, so
/// `new()` must not invent either.
#[test]
fn the_default_set_is_the_three_that_are_safe_for_any_api() {
    assert_eq!(
        sent(&SecurityHeaders::new(), false),
        [
            ("x-content-type-options".to_owned(), "nosniff".to_owned()),
            ("x-frame-options".to_owned(), "DENY".to_owned()),
            ("referrer-policy".to_owned(), "no-referrer".to_owned()),
        ]
    );
}

/// RFC 6797 section 7.2: an HSTS host "MUST NOT include the STS header field in
/// HTTP responses conveyed over non-secure transport".
///
/// The pair of cases is the requirement: configured and secure sends it,
/// configured and not secure sends nothing, and the difference is the transport
/// rather than the configuration.
#[test]
fn transport_security_rides_only_on_a_secure_transport() {
    let headers =
        SecurityHeaders::empty().strict_transport_security(Duration::from_secs(31_536_000));

    assert_eq!(
        sent(&headers, true),
        [(
            "strict-transport-security".to_owned(),
            "max-age=31536000".to_owned()
        )]
    );

    assert!(
        sent(&headers, false).is_empty(),
        "the field was sent over a transport the specification forbids it on"
    );
}

/// The two modifiers render in the order the field defines, and only when the
/// field itself was asked for.
#[test]
fn the_transport_security_modifiers_ride_on_the_field_they_modify() {
    let headers = SecurityHeaders::empty()
        .strict_transport_security(Duration::from_secs(63_072_000))
        .include_subdomains()
        .preload();

    assert_eq!(
        sent(&headers, true),
        [(
            "strict-transport-security".to_owned(),
            "max-age=63072000; includeSubDomains; preload".to_owned()
        )]
    );

    // Without the field, the modifiers have nothing to modify and invent
    // nothing.
    let bare = SecurityHeaders::empty().include_subdomains().preload();
    assert!(sent(&bare, true).is_empty());
}

/// Every name the group declares is a name some configuration can send, and
/// every name a configuration sends is declared.
///
/// The conflict check reads `NAMES`, so a name sent but undeclared is one two
/// interceptors could both set without the compiler noticing — and a name
/// declared but unsendable reserves a field nothing here writes.
#[test]
fn the_declared_names_are_exactly_the_names_that_can_be_sent() {
    let everything = SecurityHeaders::new()
        .frame_options(FrameOptions::SameOrigin)
        .referrer_policy(ReferrerPolicy::StrictOrigin)
        .permissions_policy("geolocation=()")
        .content_security_policy("default-src 'none'")
        .strict_transport_security(Duration::from_secs(1));

    let mut sent: Vec<String> = sent(&everything, true)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    sent.sort();

    let mut declared: Vec<String> = SecurityHeaderNames::NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    declared.sort();

    assert_eq!(sent, declared);
}

/// Declared, and deliberately not described.
///
/// A generated REST client does nothing with `X-Frame-Options`; the conflict
/// check still does.
#[test]
fn the_group_is_declared_and_not_described() {
    const { assert!(!SecurityHeaderNames::DESCRIBED) };
    const { assert!(!SecurityHeaderNames::NAMES.is_empty()) };
}

/// Every variant of both closed sets renders, through an exhaustive match.
///
/// Adding a variant without a field value fails to compile here.
#[test]
fn every_variant_of_both_policies_renders() {
    for options in [FrameOptions::Deny, FrameOptions::SameOrigin] {
        let expected = match options {
            FrameOptions::Deny => "DENY",
            FrameOptions::SameOrigin => "SAMEORIGIN",
        };
        assert_eq!(options.as_str(), expected);
    }

    for policy in [
        ReferrerPolicy::NoReferrer,
        ReferrerPolicy::SameOrigin,
        ReferrerPolicy::StrictOriginWhenCrossOrigin,
        ReferrerPolicy::StrictOrigin,
    ] {
        let expected = match policy {
            ReferrerPolicy::NoReferrer => "no-referrer",
            ReferrerPolicy::SameOrigin => "same-origin",
            ReferrerPolicy::StrictOriginWhenCrossOrigin => "strict-origin-when-cross-origin",
            ReferrerPolicy::StrictOrigin => "strict-origin",
        };
        assert_eq!(policy.as_str(), expected);
    }
}
