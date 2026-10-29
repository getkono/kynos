//! What a rate-limited exchange says on the wire, in both spellings.
//!
//! The header groups themselves. What a *refusal* answers with is
//! [`refusal`](super::refusal), which reads these groups to build its 429.

use std::time::Duration;

use kynos_openapi::model::schema::types::SchemaType;

use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams, UnencodableHeader},
    http::{self, whole_seconds},
    middleware::rate_limit::decision::{QuotaPolicy, ServiceLimit},
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

/// The `X-RateLimit-*` triple, in the spelling Kynos emits by default.
///
/// # Why the `X-` prefix
///
/// The unprefixed names belong to `draft-ietf-httpapi-ratelimit-headers`, which
/// has already *replaced* the triple with a single structured `RateLimit` field
/// plus `RateLimit-Policy`; these names are
/// [`DESCRIBED`](HeaderParams::DESCRIBED), so they reach generated clients.
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
    /// A service enforcing several quotas wants
    /// [`Structured`](super::Structured), which reports all of them.
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

    fn response_headers(
        registry: &mut Registry,
    ) -> kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>> {
        let _ = registry;

        // Hand-written: a derive over the `Duration` field would give a string.
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

impl EncodeHeaders for RateLimitHeaders {
    fn encode(&self) -> Result<Vec<(http::HeaderName, http::HeaderValue)>, UnencodableHeader> {
        Ok([
            ("x-ratelimit-limit", self.limit),
            ("x-ratelimit-remaining", self.remaining),
            ("x-ratelimit-reset", whole_seconds(self.reset)),
        ]
        .into_iter()
        .map(|(name, value)| {
            (
                http::HeaderName::from_static(name),
                http::HeaderValue::from(value),
            )
        })
        .collect())
    }
}

/// The `RateLimit` and `RateLimit-Policy` fields the draft defines.
///
/// Both are structured-field Lists of Items: `RateLimit-Policy` says what the
/// service enforces, `RateLimit` says where this client stands against it.
/// Unlike the `X-` triple these carry *every* quota.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateLimitFields {
    /// Where the client stands against each policy.
    pub limits: Vec<ServiceLimit>,
    /// What the service enforces.
    pub policies: Vec<QuotaPolicy>,
}

impl HeaderParams for RateLimitFields {
    const NAMES: &'static [&'static str] = &["ratelimit", "ratelimit-policy"];
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

impl EncodeHeaders for RateLimitFields {
    fn encode(&self) -> Result<Vec<(http::HeaderName, http::HeaderValue)>, UnencodableHeader> {
        let mut fields = Vec::new();

        if let Some(value) = render_limits(&self.limits) {
            fields.push((http::HeaderName::from_static("ratelimit"), value));
        }
        if let Some(value) = render_policies(&self.policies) {
            fields.push((http::HeaderName::from_static("ratelimit-policy"), value));
        }

        Ok(fields)
    }
}

/// Renders the `RateLimit` field: one member per live limit.
fn render_limits(limits: &[ServiceLimit]) -> Option<http::HeaderValue> {
    let rendered: Vec<String> = limits
        .iter()
        .filter_map(|limit| {
            use std::fmt::Write as _;

            // `r` is required, and a capped one understates what is left.
            let mut member = format!(
                "{};r={}",
                sf_string(&limit.name)?,
                limit.remaining.min(SF_INTEGER_MAX)
            );
            // `t` is optional, and a capped one would invite a retry before
            // the quota replenishes, so an unrepresentable one is omitted.
            if let Some(reset) = sf_integer(whole_seconds(limit.reset)) {
                let _ = write!(member, ";t={reset}");
            }
            Some(member)
        })
        .collect();

    http::join_names(rendered.iter().map(String::as_str))
}

/// Renders the `RateLimit-Policy` field: one member per advertised quota.
fn render_policies(policies: &[QuotaPolicy]) -> Option<http::HeaderValue> {
    let rendered: Vec<String> = policies
        .iter()
        .filter_map(|policy| {
            use std::fmt::Write as _;

            // `q` is required, and a capped one understates the allowance.
            let mut member = format!(
                "{};q={}",
                sf_string(&policy.name)?,
                policy.quota.min(SF_INTEGER_MAX)
            );
            // Section 3.1.3: "MUST be a non-negative, non-zero, Integer".
            // Rounded up, since a shorter window advertises a faster rate
            // than the one enforced; a zero window has no such integer, and
            // a capped one would be shorter, so neither is written.
            if let Some(window) = policy
                .window
                .map(whole_seconds)
                .filter(|&w| w > 0)
                .and_then(sf_integer)
            {
                let _ = write!(member, ";w={window}");
            }
            // `requests` is the draft's default. The draft's section 3.1.2 says
            // the value "MUST be a String", not a token.
            if policy.unit != crate::middleware::rate_limit::decision::QuotaUnit::Requests {
                if let Some(unit) = sf_string(policy.unit.as_str()) {
                    let _ = write!(member, ";qu={unit}");
                }
            }
            Some(member)
        })
        .collect();

    http::join_names(rendered.iter().map(String::as_str))
}

/// RFC 9651 section 3.3.1's largest `sf-integer`; a parser rejects the whole
/// field past it.
pub(super) const SF_INTEGER_MAX: u64 = 999_999_999_999_999;

/// `value` as a structured-field Integer, or `None` past its range.
fn sf_integer(value: u64) -> Option<u64> {
    (value <= SF_INTEGER_MAX).then_some(value)
}

/// Renders `name` as a structured-field String, or `None` where it cannot be
/// one.
///
/// RFC 9651 section 3.3.3: a `sf-string` is printable ASCII, and `\` and `"`
/// are escaped. An unrenderable name drops only its own member, not the field.
fn sf_string(name: &str) -> Option<String> {
    if !name.bytes().all(|byte| (0x20..0x7f).contains(&byte)) {
        return None;
    }

    let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
    Some(format!("\"{escaped}\""))
}
