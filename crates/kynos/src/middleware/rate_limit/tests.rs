use std::{fmt, time::Duration};

use super::{
    Legacy, RateLimit, Structured,
    decision::{QuotaPolicy, QuotaUnit, ServiceLimit},
    headers::{RateLimitFields, RateLimitHeaders},
    quota::{estimate, recovers_in},
    refusal::{RateLimited, RateLimitedFields, RefusalType},
};
use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams},
    response::ShortCircuit,
    schema::registry::Registry,
};

/// A window of one second, for readability.
const WINDOW: Duration = Duration::from_secs(1);

/// The estimator, over the cases a fixed window gets wrong.
///
/// A sweep of the boundary rather than a draw: the whole reason for a sliding
/// window is what happens at the seam between two of them, and that is one
/// point rather than a distribution.
#[test]
fn the_estimate_decays_the_previous_window_as_it_leaves_view() {
    // At the very start of a window the previous one counts in full.
    assert_eq!(estimate(100, 0, Duration::ZERO, WINDOW), 100);

    // Halfway through, half of it does.
    assert_eq!(estimate(100, 0, Duration::from_millis(500), WINDOW), 50);

    // At the end, none of it.
    assert_eq!(estimate(100, 0, WINDOW, WINDOW), 0);

    // The current window always counts in full.
    assert_eq!(estimate(100, 7, Duration::from_millis(500), WINDOW), 57);

    // Past the window's end the previous one is simply gone, rather than
    // counting negatively.
    assert_eq!(estimate(100, 7, Duration::from_secs(9), WINDOW), 7);
}

/// The failure a fixed window has and this does not.
///
/// A client spending its whole quota at the end of one window and its whole
/// quota at the start of the next has sent twice the rate the policy names. The
/// estimate sees it.
#[test]
fn a_burst_across_the_seam_is_counted_rather_than_forgiven() {
    let spent_last_window = 100;
    let spent_this_window = 100;

    // One millisecond into the new window, essentially all of the previous one
    // is still in view.
    let seen = estimate(
        spent_last_window,
        spent_this_window,
        Duration::from_millis(1),
        WINDOW,
    );

    assert!(
        seen > 190,
        "a fixed window would have counted {spent_this_window}; this saw {seen}"
    );
}

#[test]
fn an_estimate_over_a_zero_window_is_the_current_count() {
    assert_eq!(estimate(9, 3, Duration::ZERO, Duration::ZERO), 3);
}

/// Saturating rather than wrapping: an overflowing estimate must refuse rather
/// than wrap into permission.
#[test]
fn an_estimate_that_would_overflow_saturates() {
    assert_eq!(
        estimate(u64::MAX, u64::MAX, Duration::ZERO, WINDOW),
        u64::MAX
    );
}

/// The recovery delay is a number the service can honour.
#[test]
fn the_recovery_delay_is_when_the_estimate_falls_far_enough() {
    // Already under the headroom: the wait is just the rest of the window.
    assert_eq!(
        recovers_in(5, 10, Duration::from_millis(400), WINDOW),
        Duration::from_millis(600)
    );

    // Exactly at it: the same.
    assert_eq!(
        recovers_in(10, 10, Duration::from_millis(400), WINDOW),
        Duration::from_millis(600)
    );

    // Twice the headroom: the rest of this window, plus half of the next while
    // the carried count decays.
    assert_eq!(
        recovers_in(20, 10, Duration::ZERO, WINDOW),
        Duration::from_millis(1500)
    );

    // A delay is never zero where the window has not closed.
    assert!(recovers_in(1_000, 1, Duration::ZERO, WINDOW) > WINDOW);
}

// --- What the two spellings put on the wire -------------------------------

fn limits() -> Vec<ServiceLimit> {
    vec![
        ServiceLimit {
            name: "burst".into(),
            quota: 15,
            remaining: 12,
            reset: Duration::from_secs(1),
        },
        ServiceLimit {
            name: "daily".into(),
            quota: 10_000,
            remaining: 9_998,
            reset: Duration::from_secs(3_600),
        },
    ]
}

fn policies() -> Vec<QuotaPolicy> {
    vec![
        QuotaPolicy {
            name: "burst".into(),
            quota: 15,
            window: Some(Duration::from_secs(1)),
            unit: QuotaUnit::Requests,
        },
        QuotaPolicy {
            name: "daily".into(),
            quota: 10_000,
            window: Some(Duration::from_secs(86_400)),
            unit: QuotaUnit::ContentBytes,
        },
    ]
}

/// The field text a client actually parses.
fn rendered<G: EncodeHeaders>(group: &G) -> Vec<(String, String)> {
    group
        .encode()
        .into_iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                value.to_str().expect("a printable field").to_owned(),
            )
        })
        .collect()
}

/// The draft's structured fields, rendered per RFC 8941.
#[test]
fn the_standard_fields_render_every_quota() {
    let fields = RateLimitFields {
        limits: limits(),
        policies: policies(),
    };

    assert_eq!(
        rendered(&fields),
        [
            (
                "ratelimit".to_owned(),
                r#""burst";r=12;t=1, "daily";r=9998;t=3600"#.to_owned()
            ),
            (
                "ratelimit-policy".to_owned(),
                // `qu` is omitted for `requests`, which is the draft's default,
                // and stated for anything else -- as a String, per section
                // 3.1.2: "The value MUST be a String." A bare token parses and
                // then mis-types against a client that checks.
                r#""burst";q=15;w=1, "daily";q=10000;w=86400;qu="content-bytes""#.to_owned()
            ),
        ]
    );
}

/// The `X-` triple has room for one quota, and reports the first.
///
/// The limitation that motivates the other spelling: a limiter enforcing a
/// per-second *and* a per-day quota cannot report both here.
#[test]
fn the_legacy_triple_reports_the_first_quota_only() {
    assert_eq!(
        rendered(&RateLimitHeaders::from_limits(&limits())),
        [
            ("x-ratelimit-limit".to_owned(), "15".to_owned()),
            ("x-ratelimit-remaining".to_owned(), "12".to_owned()),
            ("x-ratelimit-reset".to_owned(), "1".to_owned()),
        ]
    );
}

/// A name no structured field can carry drops its member rather than producing
/// a field a parser rejects.
///
/// One unnameable policy must not cost the client the others.
#[test]
fn a_policy_name_that_cannot_be_a_structured_string_is_dropped() {
    let fields = RateLimitFields {
        limits: vec![
            ServiceLimit {
                name: "still\nhere".into(),
                quota: 1,
                remaining: 0,
                reset: Duration::ZERO,
            },
            ServiceLimit {
                name: "fine".into(),
                quota: 2,
                remaining: 1,
                reset: Duration::from_secs(5),
            },
        ],
        policies: Vec::new(),
    };

    assert_eq!(
        rendered(&fields),
        [("ratelimit".to_owned(), r#""fine";r=1;t=5"#.to_owned())]
    );
}

/// A quote or a backslash in a name is escaped rather than dropped.
#[test]
fn a_policy_name_carrying_a_quote_is_escaped() {
    let fields = RateLimitFields {
        limits: vec![ServiceLimit {
            name: r#"a"b\c"#.into(),
            quota: 1,
            remaining: 0,
            reset: Duration::ZERO,
        }],
        policies: Vec::new(),
    };

    assert_eq!(
        rendered(&fields)[0].1,
        r#""a\"b\\c";r=0;t=0"#,
        "RFC 8941 section 3.3.3 escapes both"
    );
}

/// Neither spelling names a field the other does, so a route carrying one is
/// never ambiguous about which it speaks.
#[test]
fn the_two_spellings_name_disjoint_fields() {
    for legacy in RateLimitHeaders::NAMES {
        assert!(
            !RateLimitFields::NAMES.contains(legacy),
            "{legacy} is claimed by both spellings"
        );
    }
}

/// A sub-second delay is reported as one second, not none.
///
/// `Retry-After` and the draft's `t` are delta-seconds, so truncating tells a
/// client to retry immediately into the refusal it just received — which the
/// example produced on its first run, at the tail of a one-second window.
#[test]
fn a_delay_shorter_than_a_second_is_still_a_delay() {
    let fields = RateLimitFields {
        limits: vec![ServiceLimit {
            name: "burst".into(),
            quota: 5,
            remaining: 0,
            reset: Duration::from_millis(400),
        }],
        policies: Vec::new(),
    };

    assert_eq!(rendered(&fields)[0].1, r#""burst";r=0;t=1"#);

    assert_eq!(
        rendered(&RateLimitHeaders {
            limit: 5,
            remaining: 0,
            reset: Duration::from_millis(400),
        })[2]
            .1,
        "1"
    );

    // A delay that really is zero stays zero: the window has closed.
    assert_eq!(
        rendered(&RateLimitHeaders {
            limit: 5,
            remaining: 5,
            reset: Duration::ZERO,
        })[2]
            .1,
        "0"
    );
}

/// Every quota unit renders as a structured-field String.
///
/// The match is exhaustive, so adding a variant to `QuotaUnit` fails to compile
/// here rather than shipping a unit that renders as a bare token — which is
/// exactly how the `content-bytes` defect reached a release.
#[test]
fn every_quota_unit_renders_as_a_string() {
    for unit in [QuotaUnit::Requests, QuotaUnit::ContentBytes] {
        // Exhaustive: a new variant makes this arm non-exhaustive.
        let expected = match unit {
            QuotaUnit::Requests => "requests",
            QuotaUnit::ContentBytes => "content-bytes",
        };
        assert_eq!(unit.as_str(), expected);

        let fields = RateLimitFields {
            limits: Vec::new(),
            policies: vec![QuotaPolicy {
                name: "p".into(),
                quota: 1,
                window: None,
                unit,
            }],
        };

        let policy = rendered(&fields)
            .into_iter()
            .find(|(name, _)| name == "ratelimit-policy")
            .map(|(_, value)| value)
            .expect("a policy field");

        if unit == QuotaUnit::Requests {
            assert!(
                !policy.contains("qu="),
                "the default unit was stated: {policy}"
            );
        } else {
            assert!(
                policy.contains(&format!(r#"qu="{expected}""#)),
                "the unit is not a structured String: {policy}"
            );
        }
    }
}

// --- The problem type a refusal names -------------------------------------

/// A type an application would name its refusals with.
struct Throttled;

impl RefusalType for Throttled {
    const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/rate-limited");
}

/// What one short circuit declares and what it sends, as the `type` of each.
///
/// `None` on the declared side means the media type carries no example at all,
/// which is what an unnamed type must leave behind.
async fn declared_and_sent<S: ShortCircuit>(value: S) -> (Option<serde_json::Value>, String) {
    use http_body_util::BodyExt as _;
    use kynos_openapi::{RefOr, model::body::mime_names::APPLICATION_PROBLEM_JSON};

    let declared = match S::responses(&mut Registry::new()).get(429) {
        Some(RefOr::Item(response)) => response
            .content
            .get(APPLICATION_PROBLEM_JSON)
            .and_then(|media_type| media_type.example())
            .and_then(|example| example.get("type"))
            .cloned(),
        _ => panic!("a refusal declares a 429 carrying a problem document"),
    };

    let body = value
        .into_response()
        .into_body()
        .collect()
        .await
        .expect("a readable body")
        .to_bytes();
    let sent: serde_json::Value = serde_json::from_slice(&body).expect("a problem document");

    (
        declared,
        sent["type"].as_str().expect("a type URI").to_owned(),
    )
}

/// One `RefusalType` is read by both halves of a refusal's promise.
///
/// Both spellings, because the URI is stated on the limiter rather than on the
/// spelling and a service choosing the draft's fields must not lose it. And
/// both *halves*, because a URI reaching only the wire leaves the document
/// saying `about:blank` about a response that says otherwise -- the
/// declaration-versus-behaviour defect the short-circuit sweep exists to catch.
#[tokio::test]
async fn a_named_refusal_type_is_one_statement_both_halves_read() {
    let uri = Throttled::TYPE_URI.expect("the marker names a type");

    let (declared, sent) =
        declared_and_sent(RateLimited::<Throttled>::new(Duration::from_secs(30), 100)).await;
    assert_eq!(
        declared.as_ref().and_then(serde_json::Value::as_str),
        Some(uri)
    );
    assert_eq!(sent, uri);

    let (declared, sent) = declared_and_sent(RateLimitedFields::<Throttled>::new(
        Duration::from_secs(30),
        limits(),
        policies(),
    ))
    .await;
    assert_eq!(
        declared.as_ref().and_then(serde_json::Value::as_str),
        Some(uri)
    );
    assert_eq!(sent, uri);
}

/// Naming no type leaves both halves exactly as they were.
///
/// The default is `()`, so this is what every document Kynos already emits
/// says: `about:blank` on the wire, and no example beside it.
#[tokio::test]
async fn an_unnamed_refusal_type_declares_no_example_and_sends_about_blank() {
    let (declared, sent) =
        declared_and_sent(RateLimited::<()>::new(Duration::from_secs(30), 100)).await;
    assert_eq!(declared, None);
    assert_eq!(sent, "about:blank");

    let (declared, sent) = declared_and_sent(RateLimitedFields::<()>::new(
        Duration::from_secs(30),
        limits(),
        policies(),
    ))
    .await;
    assert_eq!(declared, None);
    assert_eq!(sent, "about:blank");
}

// --- The implementations a refusal writes out by hand ----------------------

/// Witnesses that a refusal has all four implementations, whatever names its
/// problem type.
///
/// The bound is the whole point of writing them out: `Throttled` derives
/// nothing, so a `#[derive]` on `RateLimited` would bound `T` and refuse this
/// call.
fn assert_refusal_traits<T: Clone + fmt::Debug + Eq>() {}

/// A refusal survives a clone, in both spellings.
///
/// Hand-written rather than derived, so nothing regenerates them when a field
/// is added: `Clone` that forgot `limit` would hand back a refusal reporting a
/// ceiling of zero, and no compiler diagnostic would say so.
#[test]
fn a_cloned_refusal_carries_everything_the_original_did() {
    assert_refusal_traits::<RateLimited<Throttled>>();
    assert_refusal_traits::<RateLimitedFields<Throttled>>();

    let refusal = RateLimited::<Throttled>::new(Duration::from_secs(30), 100);
    assert_eq!(refusal.clone(), refusal);

    let fields = RateLimitedFields::<Throttled>::new(Duration::from_secs(30), limits(), policies());
    assert_eq!(fields.clone(), fields);
}

/// Every field a refusal carries is a field its equality reads.
///
/// One case per field rather than one for the struct: an `eq` that compares
/// three of four members agrees with itself on every value that differs only in
/// the fourth, so a draw would pass while the omission stood.
#[test]
fn two_refusals_differing_in_one_field_are_unequal() {
    let refusal = RateLimited::<Throttled>::new(Duration::from_secs(30), 100);

    assert_ne!(
        refusal,
        RateLimited::new(Duration::from_secs(31), 100),
        "retry_after is not compared"
    );
    assert_ne!(
        refusal,
        RateLimited::new(Duration::from_secs(30), 101),
        "limit is not compared"
    );

    let fields = RateLimitedFields::<Throttled>::new(Duration::from_secs(30), limits(), policies());

    assert_ne!(
        fields,
        RateLimitedFields::new(Duration::from_secs(31), limits(), policies()),
        "retry_after is not compared"
    );
    assert_ne!(
        fields,
        RateLimitedFields::new(Duration::from_secs(30), Vec::new(), policies()),
        "limits is not compared"
    );
    assert_ne!(
        fields,
        RateLimitedFields::new(Duration::from_secs(30), limits(), Vec::new()),
        "policies is not compared"
    );
}

/// Every field a refusal carries is a field its `Debug` prints.
///
/// A refusal is what an operator reads out of a log when a client complains it
/// was throttled, and a member the formatter skips is one that is simply not
/// there. The marker is deliberately absent: it is a name rather than a value,
/// and `PhantomData<fn() -> T>` prints nothing an operator can use.
#[test]
fn a_refusal_prints_every_field_it_carries() {
    assert_eq!(
        format!(
            "{:?}",
            RateLimited::<Throttled>::new(Duration::from_secs(30), 100)
        ),
        "RateLimited { retry_after: 30s, limit: 100 }"
    );

    let printed = format!(
        "{:?}",
        RateLimitedFields::<Throttled>::new(Duration::from_secs(30), limits(), policies())
    );

    assert!(printed.starts_with("RateLimitedFields {"), "{printed}");
    assert!(printed.contains("retry_after: 30s"), "{printed}");
    assert!(
        printed.contains(r#"limits: [ServiceLimit { name: "burst""#),
        "{printed}"
    );
    assert!(
        printed.contains(r#"policies: [QuotaPolicy { name: "burst""#),
        "{printed}"
    );
}

// --- The implementations a limiter keeps ----------------------------------

/// A policy stand-in. Only the struct's own bounds are under test here, so this
/// does not have to be a `RateLimitPolicy`.
#[derive(Clone, Debug)]
struct Policy;

/// Witnesses that a limiter has both implementations.
fn assert_clone_and_debug<T: Clone + fmt::Debug>() {}

/// A limiter keeps `Clone` and `Debug` whatever type names its refusal.
///
/// `#[derive]` bounds every parameter, so it demanded `T: Clone + Debug` from a
/// marker that is a name and never a value -- taking both implementations away
/// from any limiter naming a problem type, which is exactly the cost the
/// refusal spends eight hand-written implementations to avoid one type down.
/// `Throttled` derives nothing, so this call is the whole test.
#[test]
fn a_limiter_keeps_its_impls_whatever_type_names_its_refusal() {
    assert_clone_and_debug::<RateLimit<Policy, Legacy, ()>>();
    assert_clone_and_debug::<RateLimit<Policy, Structured, Throttled>>();

    // And they still say what they said: the spelling and the problem type are
    // in the type name, so `Debug` has one field to print.
    let limiter = RateLimit::new(Policy)
        .refusal_type::<Throttled>()
        .standard_fields();

    assert_eq!(
        format!("{:?}", limiter.clone()),
        "RateLimit { policy: Policy }"
    );
}
