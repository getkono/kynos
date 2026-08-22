//! What a rate-limited exchange says on the wire, in both spellings.

use std::time::Duration;

use kynos_openapi::model::schema::types::SchemaType;

use crate::{
    error::problem::Problem,
    extract::params::header::HeaderParams,
    http,
    middleware::rate_limit::decision::{QuotaPolicy, ServiceLimit},
    response::{IntoResponse, Responses, ShortCircuit},
    schema::registry::Registry,
};

/// Describes one integer count.
fn count(description: &str) -> kynos_openapi::RefOr<kynos_openapi::Header> {
    kynos_openapi::RefOr::Item(
        kynos_openapi::Header::new(kynos_openapi::Schema::of_type(SchemaType::Integer))
            .with_description(description),
    )
}

/// Describes one structured field, which is a string however it is built.
fn structured(description: &str) -> kynos_openapi::RefOr<kynos_openapi::Header> {
    kynos_openapi::RefOr::Item(
        kynos_openapi::Header::new(kynos_openapi::Schema::of_type(SchemaType::String))
            .with_description(description),
    )
}

/// Describes `Retry-After`, which is a delta-seconds count or an HTTP-date.
fn retry_after_header() -> kynos_openapi::Header {
    kynos_openapi::Header::new(kynos_openapi::Schema::of_type(SchemaType::String))
        .with_description("How long to wait before retrying, in seconds or as an HTTP-date")
}

/// The `X-RateLimit-*` triple, in the spelling Kynos emits by default.
///
/// # Why the `X-` prefix
///
/// The unprefixed names belong to `draft-ietf-httpapi-ratelimit-headers`, which
/// has already *replaced* the triple with a single structured `RateLimit` field
/// plus `RateLimit-Policy`. These names are
/// [`DESCRIBED`](HeaderParams::DESCRIBED), so they reach generated clients —
/// which makes squatting names a working group is still revising expensive
/// rather than cosmetic.
///
/// [`RateLimit::standard_fields`](super::RateLimit::standard_fields) is the
/// other spelling, for a service that has decided the draft is settled enough.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateLimitHeaders {
    /// The ceiling the limit was configured with.
    pub limit: u64,
    /// Requests remaining in the current window.
    pub remaining: u64,
    /// How long until the window resets.
    pub reset: Duration,
}

impl RateLimitHeaders {
    /// The triple, reported from the first limit a policy consulted.
    ///
    /// One triple however many quotas were checked, because the spelling has
    /// room for one. A service enforcing several wants
    /// [`Structured`](super::Structured), which has room for all of them.
    pub(super) fn from_limits(limits: &[ServiceLimit]) -> Self {
        limits.first().map_or(
            Self {
                limit: 0,
                remaining: 0,
                reset: Duration::ZERO,
            },
            |limit| Self {
                limit: limit.quota,
                remaining: limit.remaining,
                reset: limit.reset,
            },
        )
    }
}

impl HeaderParams for RateLimitHeaders {
    const NAMES: &'static [&'static str] = &[
        "x-ratelimit-limit",
        "x-ratelimit-remaining",
        "x-ratelimit-reset",
    ];

    fn encode(&self) -> Vec<(http::HeaderName, http::HeaderValue)> {
        [
            ("x-ratelimit-limit", self.limit),
            ("x-ratelimit-remaining", self.remaining),
            ("x-ratelimit-reset", self.reset.as_secs()),
        ]
        .into_iter()
        .filter_map(|(name, value)| {
            Some((
                http::HeaderName::from_static(name),
                http::HeaderValue::from_str(&value.to_string()).ok()?,
            ))
        })
        .collect()
    }

    fn response_headers(
        registry: &mut Registry,
    ) -> kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>> {
        let _ = registry;

        // Hand-written rather than derived: each of the three is a count, and a
        // count is an integer — where a derive over a `Duration` field would
        // give a string.
        let mut headers = kynos_openapi::Map::new();
        headers.insert(
            "X-RateLimit-Limit".to_owned(),
            count("Requests permitted per window"),
        );
        headers.insert(
            "X-RateLimit-Remaining".to_owned(),
            count("Requests remaining in the current window"),
        );
        headers.insert(
            "X-RateLimit-Reset".to_owned(),
            count("Seconds until the current window resets"),
        );
        headers
    }
}

/// The `RateLimit` and `RateLimit-Policy` fields the draft defines.
///
/// Both are structured-field Lists of Items: `RateLimit-Policy` says what the
/// service enforces, `RateLimit` says where this client stands against it.
/// Unlike the `X-` triple these carry *every* quota, which is what makes a
/// limiter with a per-second and a per-day window reportable at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateLimitFields {
    /// Where the client stands against each policy.
    pub limits: Vec<ServiceLimit>,
    /// What the service enforces.
    pub policies: Vec<QuotaPolicy>,
}

impl HeaderParams for RateLimitFields {
    const NAMES: &'static [&'static str] = &["ratelimit", "ratelimit-policy"];

    fn encode(&self) -> Vec<(http::HeaderName, http::HeaderValue)> {
        let mut fields = Vec::new();

        if let Some(value) = render_limits(&self.limits) {
            fields.push((http::HeaderName::from_static("ratelimit"), value));
        }
        if let Some(value) = render_policies(&self.policies) {
            fields.push((http::HeaderName::from_static("ratelimit-policy"), value));
        }

        fields
    }

    fn response_headers(
        registry: &mut Registry,
    ) -> kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>> {
        let _ = registry;

        let mut headers = kynos_openapi::Map::new();
        headers.insert(
            "RateLimit".to_owned(),
            structured(
                "Where this client stands against each policy, as a structured-field List: \
                 `\"name\";r=<remaining>;t=<seconds>`",
            ),
        );
        headers.insert(
            "RateLimit-Policy".to_owned(),
            structured(
                "What the service enforces, as a structured-field List: \
                 `\"name\";q=<quota>;w=<seconds>;qu=<unit>`",
            ),
        );
        headers
    }
}

/// Renders the `RateLimit` field: one member per live limit.
fn render_limits(limits: &[ServiceLimit]) -> Option<http::HeaderValue> {
    let rendered: Vec<String> = limits
        .iter()
        .filter_map(|limit| {
            Some(format!(
                "{};r={};t={}",
                sf_string(&limit.name)?,
                limit.remaining,
                limit.reset.as_secs()
            ))
        })
        .collect();

    (!rendered.is_empty())
        .then(|| http::HeaderValue::from_str(&rendered.join(", ")).ok())
        .flatten()
}

/// Renders the `RateLimit-Policy` field: one member per advertised quota.
fn render_policies(policies: &[QuotaPolicy]) -> Option<http::HeaderValue> {
    let rendered: Vec<String> = policies
        .iter()
        .filter_map(|policy| {
            use std::fmt::Write as _;

            let mut member = format!("{};q={}", sf_string(&policy.name)?, policy.quota);
            if let Some(window) = policy.window {
                // Writing into the string rather than allocating another to
                // append; the members are built once per response.
                let _ = write!(member, ";w={}", window.as_secs());
            }
            // `requests` is the draft's default, so stating it says nothing.
            if policy.unit != crate::middleware::rate_limit::decision::QuotaUnit::Requests {
                let _ = write!(member, ";qu={}", policy.unit.as_str());
            }
            Some(member)
        })
        .collect();

    (!rendered.is_empty())
        .then(|| http::HeaderValue::from_str(&rendered.join(", ")).ok())
        .flatten()
}

/// Renders `name` as a structured-field String, or `None` where it cannot be
/// one.
///
/// RFC 8941 section 3.3.3: a `sf-string` is printable ASCII, and `\` and `"`
/// are escaped. A name that cannot be rendered drops its member rather than
/// producing a field a parser will reject — one unnameable policy must not cost
/// the client the others.
fn sf_string(name: &str) -> Option<String> {
    if !name.bytes().all(|byte| (0x20..0x7f).contains(&byte)) {
        return None;
    }

    let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
    Some(format!("\"{escaped}\""))
}

/// What a limiter answers with when a policy refuses, in the `X-` spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateLimited {
    /// How long the client should wait before retrying.
    pub retry_after: Duration,
    /// The ceiling that was exceeded.
    pub limit: u64,
}

impl IntoResponse for RateLimited {
    fn into_response(self) -> http::Response {
        let mut response = refusal();
        set_retry_after(&mut response, self.retry_after);

        // The same three a success carries. A denial's reset *is* its retry
        // delay, so reporting it lands no new obligation on the policy.
        write_group(
            &mut response,
            &RateLimitHeaders {
                limit: self.limit,
                remaining: 0,
                reset: self.retry_after,
            },
        );

        response
    }
}

impl ShortCircuit for RateLimited {
    const STATUSES: &'static [u16] = &[429];
}

impl Responses for RateLimited {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        described_refusal(RateLimitHeaders::response_headers(registry))
    }
}

/// The same, in the draft's spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateLimitedFields {
    /// How long the client should wait before retrying.
    pub retry_after: Duration,
    /// Where the client stands against each policy.
    pub limits: Vec<ServiceLimit>,
    /// What the service enforces.
    pub policies: Vec<QuotaPolicy>,
}

impl IntoResponse for RateLimitedFields {
    fn into_response(self) -> http::Response {
        let mut response = refusal();
        set_retry_after(&mut response, self.retry_after);
        write_group(
            &mut response,
            &RateLimitFields {
                limits: self.limits,
                policies: self.policies,
            },
        );
        response
    }
}

impl ShortCircuit for RateLimitedFields {
    const STATUSES: &'static [u16] = &[429];
}

impl Responses for RateLimitedFields {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        described_refusal(RateLimitFields::response_headers(registry))
    }
}

/// The 429 both spellings share.
fn refusal() -> http::Response {
    Problem::new(http::StatusCode::TOO_MANY_REQUESTS)
        .with_detail("the client has exceeded its request rate")
        .into_response()
}

/// The 429's description, plus whichever header group produced it.
fn described_refusal(
    group: kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>>,
) -> kynos_openapi::Responses {
    kynos_openapi::Responses::new().with(
        429,
        group.into_iter().fold(
            kynos_openapi::Response::new("the client has exceeded its request rate")
                .with_header("Retry-After", retry_after_header()),
            |response, (name, header)| match header {
                kynos_openapi::RefOr::Item(header) => response.with_header(name, header),
                kynos_openapi::RefOr::Ref(_) => response,
            },
        ),
    )
}

fn set_retry_after(response: &mut http::Response, retry_after: Duration) {
    if let Ok(value) = http::HeaderValue::from_str(&retry_after.as_secs().to_string()) {
        response
            .headers_mut()
            .insert(http::header::RETRY_AFTER, value);
    }
}

/// Writes a group onto a short-circuit response.
///
/// Through the one writer, so a short circuit and a forwarded response spell a
/// group the same way.
fn write_group<G: HeaderParams>(response: &mut http::Response, group: &G) {
    crate::extract::params::header::write(response.headers_mut(), group);
}
