use std::{fmt, marker::PhantomData, time::Duration};

use super::{
    Legacy, RateLimit, RateLimitSpelling, Structured,
    decision::{Decision, Denial, QuotaPolicy, QuotaUnit, RateLimitPolicy, ServiceLimit},
    headers::{RateLimitFields, RateLimitHeaders},
    key::Shared,
    quota::{Quota, Quotas, estimate, recovers_in},
    refusal::{RateLimited, RateLimitedFields},
    store::{RateLimitStore, StoreFailure},
};
use kynos_openapi::{Method, RefOr};

use crate::{
    error::problem::ProblemType,
    extract::params::header::{EncodeHeaders, HeaderParams},
    http::body::Body,
    response::{IntoResponse, ShortCircuit},
    router::operation::Route,
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
        .expect("a rate-limit group always encodes")
        .into_iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                value.to_str().expect("a printable field").to_owned(),
            )
        })
        .collect()
}

/// The draft's structured fields, rendered per RFC 9651.
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

/// The `X-RateLimit-Limit` a `Legacy` 429 sends for `limits`.
fn refused_ceiling(retry_after: Duration, limits: &[ServiceLimit]) -> String {
    let refusal = <Legacy as RateLimitSpelling<()>>::deny(retry_after, limits, &[]);
    let response = refusal.into_response();

    response.headers()["x-ratelimit-limit"]
        .to_str()
        .expect("a printable field")
        .to_owned()
}

/// A `Legacy` 429 reports the ceiling that was exceeded, not the first one
/// consulted.
///
/// A burst quota with room left beside a daily one that refused: the burst
/// ceiling would tell the client it may send ten more a second, when the daily
/// quota will refuse every one of them.
#[test]
fn a_legacy_refusal_reports_the_quota_that_refused() {
    let day = Duration::from_secs(86_400);
    let limits = [
        ServiceLimit::new("burst", 10, 9, Duration::from_secs(1)),
        ServiceLimit::new("daily", 1_000, 0, day),
    ];

    assert_eq!(refused_ceiling(day, &limits), "1000");
}

/// Of two quotas that refused, the one the client waits longest on is the one
/// `Retry-After` and `X-RateLimit-Reset` describe, so its ceiling is reported.
#[test]
fn a_legacy_refusal_reports_the_refusing_quota_with_the_longest_reset() {
    let hour = Duration::from_secs(3_600);
    let limits = [
        ServiceLimit::new("burst", 10, 0, Duration::from_secs(1)),
        ServiceLimit::new("hourly", 100, 0, hour),
        ServiceLimit::new("minutely", 50, 0, Duration::from_secs(60)),
    ];

    assert_eq!(refused_ceiling(hour, &limits), "100");

    // Of equal waits, the first consulted.
    let tied = [
        ServiceLimit::new("hourly", 100, 0, hour),
        ServiceLimit::new("tenants", 400, 0, hour),
    ];
    assert_eq!(refused_ceiling(hour, &tied), "100");
}

/// A policy may refuse without reporting any limit as exhausted; the first
/// one consulted is then the only ceiling there is to report.
#[test]
fn a_legacy_refusal_with_no_exhausted_quota_reports_the_first() {
    let limits = [
        ServiceLimit::new("burst", 10, 3, Duration::from_secs(1)),
        ServiceLimit::new("daily", 1_000, 7, Duration::from_secs(86_400)),
    ];

    assert_eq!(refused_ceiling(Duration::from_secs(5), &limits), "10");
    assert_eq!(refused_ceiling(Duration::from_secs(5), &[]), "0");
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
        "RFC 9651 section 3.3.3 escapes both"
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

/// The longest delay saturates at `u64::MAX` seconds rather than wrapping.
///
/// A [`RateLimitPolicy`](super::decision::RateLimitPolicy) may answer
/// `Duration::MAX` for "never". Rounding its last part-second up overflows
/// `u64`: a debug build panics, and a release build wraps to `0`, telling the
/// client to retry at once.
#[test]
fn the_longest_delay_saturates_rather_than_wrapping() {
    let fields = RateLimitFields {
        limits: vec![ServiceLimit {
            name: "never".into(),
            quota: 5,
            remaining: 0,
            reset: Duration::MAX,
        }],
        policies: Vec::new(),
    };

    assert_eq!(
        rendered(&fields)[0].1,
        format!(r#""never";r=0;t={}"#, u64::MAX)
    );

    assert_eq!(
        rendered(&RateLimitHeaders {
            limit: 5,
            remaining: 0,
            reset: Duration::MAX,
        })[2]
            .1,
        u64::MAX.to_string()
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

impl ProblemType for Throttled {
    const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/rate-limited");
}

/// What one short circuit's 429 declares its `type` may be, and what the wire
/// carried.
///
/// The declared half is read out of the *schema*, which is the half a body is
/// validated against: `assert_conformance` compiles the declared schema and
/// runs a response through it, and never reads an `example`.
async fn declared_and_sent<S: ShortCircuit>(value: S) -> (String, String) {
    use http_body_util::BodyExt as _;

    let declared = match S::responses(&mut Registry::new()).get(429) {
        Some(RefOr::Item(response)) => declared_type(response),
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

/// The one URI a declared problem response narrows `type` to.
///
/// The shape [`narrowed_response`](crate::error::problem) builds for a single
/// branch: the shared component, and one object fixing `type` to a `const`.
/// Anything else means the narrowing was lost, so this panics rather than
/// returning an `Option` a caller could compare away.
fn declared_type(response: &kynos_openapi::Response) -> String {
    use kynos_openapi::model::body::mime_names::APPLICATION_PROBLEM_JSON;

    let schema = response
        .content
        .get(APPLICATION_PROBLEM_JSON)
        .and_then(RefOr::as_item)
        .and_then(|media_type| media_type.schema.as_ref())
        .expect("a declared problem document carries a schema");

    let declared = serde_json::to_value(schema).expect("a schema is serializable");

    declared["allOf"][1]["properties"]["type"]["const"]
        .as_str()
        .unwrap_or_else(|| panic!("the declared 429 does not narrow `type`: {declared}"))
        .to_owned()
}

/// One `ProblemType` is read by both halves of a refusal's promise.
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
    assert_eq!(declared, uri);
    assert_eq!(sent, uri);

    let (declared, sent) = declared_and_sent(RateLimitedFields::<Throttled>::new(
        Duration::from_secs(30),
        limits(),
        policies(),
    ))
    .await;
    assert_eq!(declared, uri);
    assert_eq!(sent, uri);
}

/// The narrowing fixes `type` and leaves every other member alone.
///
/// `title` and `detail` are English prose. A localizing interceptor rewrites
/// them per request -- `examples/localized_errors.rs` is exactly that -- so a
/// declaration that also fixed them would fail a response saying `"Trop de
/// requetes"`, and this time the harness *would* notice: a narrowed schema is
/// what `assert_conformance` validates a body against. So the assertion is
/// that the narrowing adds one property and no other.
#[tokio::test]
async fn the_narrowing_fixes_the_one_member_the_wire_cannot_vary() {
    let declared = match <RateLimited<Throttled> as crate::response::Responses>::responses(
        &mut Registry::new(),
    )
    .get(429)
    {
        Some(RefOr::Item(response)) => serde_json::to_value(
            response
                .content
                .get(kynos_openapi::model::body::mime_names::APPLICATION_PROBLEM_JSON)
                .and_then(RefOr::as_item)
                .and_then(|media_type| media_type.schema.as_ref())
                .expect("a declared problem document carries a schema"),
        )
        .expect("a schema is serializable"),
        _ => panic!("a refusal declares a 429 carrying a problem document"),
    };

    let narrowed = declared["allOf"][1]["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("the declared 429 does not narrow anything: {declared}"))
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();

    assert_eq!(narrowed, ["type"], "{declared}");
}

/// Naming no type states `about:blank` on both halves rather than neither.
///
/// The default is `()`, and what it declares is the URI `Problem::new` really
/// sets -- not a bare `$ref` to the shared component. That distinction is the
/// one `docs/errors.md` draws for a rejection and it holds here for the same
/// reason: a `$ref` admits every problem document the service can produce, so a
/// status an extractor also claims would lose its own narrowing to this one.
#[tokio::test]
async fn an_unnamed_refusal_type_declares_and_sends_about_blank() {
    let (declared, sent) =
        declared_and_sent(RateLimited::<()>::new(Duration::from_secs(30), 100)).await;
    assert_eq!(declared, "about:blank");
    assert_eq!(sent, "about:blank");

    let (declared, sent) = declared_and_sent(RateLimitedFields::<()>::new(
        Duration::from_secs(30),
        limits(),
        policies(),
    ))
    .await;
    assert_eq!(declared, "about:blank");
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

/// Witnesses that a refusal crosses a task boundary whatever names its type.
fn assert_send_sync<T: Send + Sync>() {}

/// A marker that is deliberately neither `Send` nor `Sync`.
///
/// A raw pointer is the cheapest way to be neither. It is still `'static`, so
/// it satisfies `ProblemType` and the only thing under test is whether the
/// refusal's auto traits followed it.
struct Unsendable(PhantomData<*const ()>);

impl ProblemType for Unsendable {
    const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/unsendable");
}

/// A refusal is `Send` and `Sync` whatever marker names its problem type.
///
/// The field is `PhantomData<fn() -> T>` rather than `PhantomData<T>` for this
/// reason and no other: a `PhantomData<T>` inherits `T`'s auto traits, and a
/// refusal that is not `Send` cannot be returned from an interceptor at all --
/// a bound failure at every mount site, from a marker the application thought
/// was only a name.
#[test]
fn a_refusal_is_send_and_sync_whatever_marker_names_it() {
    assert_send_sync::<RateLimited<Unsendable>>();
    assert_send_sync::<RateLimitedFields<Unsendable>>();
}

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

    // And they still say what they said. One field: `_spelling` holds nothing
    // an operator can read.
    let limiter = RateLimit::new(Policy)
        .problem_type::<Throttled>()
        .standard_fields();

    assert_eq!(
        format!("{:?}", limiter.clone()),
        "RateLimit { policy: Policy }"
    );
}

// --- The wait a refusal reports -------------------------------------------

/// A store that cannot answer for the quota named `unreachable`, and reads one
/// request already counted under every other key.
#[derive(Clone, Copy, Debug)]
struct PartlyDown;

#[derive(Debug, thiserror::Error)]
#[error("the counter store is unavailable")]
struct Unavailable;

impl RateLimitStore for PartlyDown {
    type Error = Unavailable;

    async fn read(&self, key: &str) -> Result<u64, Self::Error> {
        if key.contains("|unreachable|") {
            Err(Unavailable)
        } else {
            Ok(1)
        }
    }

    async fn increment(&self, _: &str, by: u64, _: Duration) -> Result<u64, Self::Error> {
        Ok(1 + by)
    }
}

/// The wait `quotas` reports for one request, with every limit it reported.
async fn denied(quotas: Quotas<Shared, PartlyDown>) -> Denial {
    let request = http::Request::new(Body::empty());
    let route = Route::new("/", "op", Method::Get);

    match quotas.check(&request, route, &()).await {
        Decision::Deny(denial) => denial,
        Decision::Allow(allowance) => panic!("one quota refuses, yet {allowance:?}"),
    }
}

/// A refusal reports the wait after which every quota would admit the request,
/// whichever order the quotas were added in and whichever of them denied
/// because the store could not answer.
///
/// `refused` is spent and recovers within two seconds. `unreachable`'s window
/// is long enough that what remains of it outlasts that at any instant before
/// 2106, so the two waits differ in the direction an order-dependent fold
/// would get wrong.
#[tokio::test]
async fn a_refusal_waits_out_the_longest_denial_in_either_quota_order() {
    let refused = || Quota::new("refused", 1, Duration::from_secs(1));
    let unreachable = || Quota::new("unreachable", 1, Duration::from_secs(u64::from(u32::MAX)));
    let quotas =
        || Quotas::new(Shared("everyone".into()), PartlyDown).on_store_failure(StoreFailure::Deny);

    for denial in [
        denied(quotas().quota(refused()).quota(unreachable())).await,
        denied(quotas().quota(unreachable()).quota(refused())).await,
    ] {
        let longest = denial
            .limits
            .iter()
            .map(|limit| limit.reset)
            .max()
            .expect("both quotas report");

        assert!(longest > Duration::from_secs(2), "{denial:?}");
        assert_eq!(denial.retry_after, longest, "{denial:?}");
    }
}

// --- What the shipped keys and quotas state --------------------------------

/// The partition `key` gives a request carrying `fields`.
fn partition_of<K: super::key::RateLimitKey<()>>(
    key: &K,
    fields: &[(&str, &[u8])],
) -> Option<String> {
    let mut request = crate::http::Request::new(Body::empty());
    for (name, value) in fields {
        request.headers_mut().append(
            crate::http::HeaderName::from_bytes(name.as_bytes()).expect("a legal field name"),
            crate::http::HeaderValue::from_bytes(value).expect("a legal field value"),
        );
    }
    key.partition(
        &request,
        Route::new("/counted", "counted", Method::Get),
        &(),
    )
    .map(std::borrow::Cow::into_owned)
}

/// A request without the field, or with one that is not text, counts against
/// the one shared `none` bucket rather than escaping the limit — so removing
/// the header, or garbling it, is no way past it.
#[test]
fn a_header_key_puts_every_request_without_a_readable_value_in_one_bucket() {
    let key = super::key::ByHeader(crate::http::HeaderName::from_static("x-tenant"));

    assert_eq!(
        partition_of(&key, &[("x-tenant", b"acme")]).as_deref(),
        Some("x-tenant:acme")
    );
    assert_eq!(partition_of(&key, &[]).as_deref(), Some("x-tenant:none"));
    assert_eq!(
        partition_of(&key, &[("x-tenant", b"\xff")]).as_deref(),
        Some("x-tenant:none")
    );
}

/// An IPv4 peer is one bucket whichever listener it reached: a dual-stack
/// `[::]` listener reports it IPv4-mapped (RFC 4291 section 2.5.5.2), as
/// `Forwarded::client` already reads it.
#[test]
fn a_peer_key_names_an_ipv4_mapped_peer_by_its_ipv4_host() {
    use std::net::{Ipv4Addr, SocketAddr};

    let local = SocketAddr::from((Ipv4Addr::LOCALHOST, 8080));
    let partition = |peer: SocketAddr| {
        let mut request = crate::http::Request::new(Body::empty());
        request
            .extensions_mut()
            .insert(crate::extract::connection::Connection::from_peer(
                peer, local,
            ));
        super::key::RateLimitKey::<()>::partition(
            &super::key::ByPeerAddress,
            &request,
            Route::new("/counted", "counted", Method::Get),
            &(),
        )
        .map(std::borrow::Cow::into_owned)
    };

    let host = Ipv4Addr::new(203, 0, 113, 7);
    let mapped = SocketAddr::from((host.to_ipv6_mapped(), 50_000));
    let plain = SocketAddr::from((host, 50_001));

    assert_eq!(partition(mapped).as_deref(), Some("peer:203.0.113.7"));
    assert_eq!(partition(plain).as_deref(), Some("peer:203.0.113.7"));
}

/// `And` joins two partitions, and either half exempting the request exempts
/// it: a key with a missing part is not a key.
#[test]
fn a_joined_key_is_exempt_when_either_half_is() {
    use super::key::And;

    let exempt = |_: &crate::http::Request,
                  _: Route<'_>,
                  (): &()|
     -> Option<std::borrow::Cow<'static, str>> { None };

    let a = || Shared("a".into());
    let b = || Shared("b".into());

    assert_eq!(partition_of(&And(a(), b()), &[]).as_deref(), Some("a|b"));
    assert_eq!(partition_of(&And(a(), exempt), &[]), None);
    assert_eq!(partition_of(&And(exempt, b()), &[]), None);
}

/// A closure is a key: the partition it returns is the one counted against.
#[test]
fn a_closure_keys_by_the_partition_it_returns() {
    let tenant = |_: &crate::http::Request,
                  _: Route<'_>,
                  (): &()|
     -> Option<std::borrow::Cow<'static, str>> { Some("tenant:acme".into()) };

    assert_eq!(partition_of(&tenant, &[]).as_deref(), Some("tenant:acme"));
}

/// The peer key names the connection's IP and not its port, so a client's
/// every connection shares one bucket and another client's does not; a request
/// that arrived on no socket counts against the shared `peer:none` bucket.
#[test]
fn the_peer_key_names_the_peer_ip() {
    use crate::extract::connection::Connection;

    let from = |peer: &str| {
        let mut request = crate::http::Request::new(Body::empty());
        request.extensions_mut().insert(Connection::from_peer(
            peer.parse().expect("a socket address"),
            "192.0.2.2:443".parse().expect("a socket address"),
        ));
        super::key::RateLimitKey::<()>::partition(
            &super::key::ByPeerAddress,
            &request,
            Route::new("/counted", "counted", Method::Get),
            &(),
        )
        .map(std::borrow::Cow::into_owned)
    };

    assert_eq!(
        from("203.0.113.7:50000").as_deref(),
        Some("peer:203.0.113.7")
    );
    assert_eq!(
        from("203.0.113.7:50001").as_deref(),
        Some("peer:203.0.113.7")
    );
    assert_eq!(
        from("[2001:db8::1]:50000").as_deref(),
        Some("peer:2001:db8::1")
    );
    assert_eq!(
        partition_of(&super::key::ByPeerAddress, &[]).as_deref(),
        Some("peer:none")
    );
}

/// Each named constructor advertises the window its name says.
#[test]
fn each_named_quota_advertises_the_window_its_name_says() {
    for (quota, seconds) in [
        (Quota::per_second("q", 1), 1),
        (Quota::per_minute("q", 1), 60),
        (Quota::per_hour("q", 1), 3_600),
        (Quota::per_day("q", 1), 86_400),
    ] {
        assert_eq!(
            quota.policy().window,
            Some(Duration::from_secs(seconds)),
            "{quota:?}"
        );
    }
}

/// A policy stating no window omits `w` rather than writing a zero.
#[test]
fn a_policy_with_no_window_renders_no_w_parameter() {
    let fields = RateLimitFields {
        limits: Vec::new(),
        policies: vec![QuotaPolicy::new("open", 5, None, QuotaUnit::Requests)],
    };

    assert_eq!(
        rendered(&fields),
        [("ratelimit-policy".to_owned(), r#""open";q=5"#.to_owned())]
    );
}

// --- The whole 429 ---------------------------------------------------------

/// Every field a refusal's response carries, in the order it carries them,
/// and its problem document.
async fn sent<S: ShortCircuit>(refusal: S) -> (Vec<(String, String)>, serde_json::Value) {
    use http_body_util::BodyExt as _;

    let response = refusal.into_response();
    assert_eq!(
        response.status(),
        crate::http::StatusCode::TOO_MANY_REQUESTS
    );

    let fields = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                value.to_str().expect("a printable field").to_owned(),
            )
        })
        .collect();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("a readable body")
        .to_bytes();

    (
        fields,
        serde_json::from_slice(&body).expect("a problem document"),
    )
}

/// Both spellings' 429, whole: the problem document, `Retry-After`, and the
/// group each spelling reports — the `X-` triple at zero remaining with the
/// retry delay as its reset, or the draft's two fields over every quota.
///
/// The two are built by separate, near-identical code, so each is held to the
/// whole response rather than to the fields another test happened to read.
#[tokio::test]
async fn each_spelling_sends_its_whole_429() {
    let pairs = |fields: &[(&str, &str)]| -> Vec<(String, String)> {
        fields
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    };

    let (fields, document) = sent(RateLimited::<()>::new(Duration::from_secs(30), 100)).await;
    assert_eq!(
        fields,
        pairs(&[
            ("content-type", "application/problem+json"),
            ("retry-after", "30"),
            ("x-ratelimit-limit", "100"),
            ("x-ratelimit-remaining", "0"),
            ("x-ratelimit-reset", "30"),
        ])
    );
    assert_eq!(
        document,
        serde_json::json!({
            "type": "about:blank",
            "title": "Too Many Requests",
            "status": 429,
            "detail": "the client has exceeded its request rate",
        })
    );

    let (fields, document) = sent(RateLimitedFields::<Throttled>::new(
        Duration::from_secs(30),
        limits(),
        policies(),
    ))
    .await;
    assert_eq!(
        fields,
        pairs(&[
            ("content-type", "application/problem+json"),
            ("retry-after", "30"),
            ("ratelimit", r#""burst";r=12;t=1, "daily";r=9998;t=3600"#),
            (
                "ratelimit-policy",
                r#""burst";q=15;w=1, "daily";q=10000;w=86400;qu="content-bytes""#,
            ),
        ])
    );
    assert_eq!(
        document,
        serde_json::json!({
            "type": "https://errors.example.com/rate-limited",
            "title": "Too Many Requests",
            "status": 429,
            "detail": "the client has exceeded its request rate",
        })
    );
}
