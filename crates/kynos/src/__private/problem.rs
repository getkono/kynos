//! What `#[derive(ApiError)]` declares each status with, joins a `const` type
//! URI prefix with, reads a transparent variant's held error through and
//! unions its statuses with, sends its
//! extension members through, declares a constant one by, and sends and
//! declares a failure's
//! `#[problem(headers)]` group and `#[problem(retry_after)]` delay with, and
//! logs a 5xx failure through before its problem consumes it.
//!
//! [`response`] forwards to [`error::problem`](crate::error::problem), where
//! the shape is built, and the header helpers to
//! [`response::headers`](crate::response::headers), where `WithHeaders` writes
//! and declares its own; a `pub use` would give each a second path. The 5xx
//! event is emitted by `middleware::trace::reported`, next to the request
//! log, so the logging facade stays named where the runtime allowance lists.

use std::time::Duration;

use kynos_openapi::{Header, Map, RefOr, Response, Responses, Schema as OpenApiSchema};

use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams},
    http::retry_after::RetryAfter,
    schema::registry::Registry,
};

/// The response one status declares, narrowed to the types its failures
/// publish.
///
/// Each branch is the type URI a failure publishes, or `None` where it names
/// none, the summary its declaration gave it, and its extension members: each
/// one's name, its type's schema, and whether every occurrence sends it.
/// `described` is the enum's `describe` text for the status, if it wrote one.
#[must_use]
pub fn response(
    problem: &OpenApiSchema,
    status: u16,
    described: Option<&'static str>,
    branches: &[crate::error::problem::Branch<'_>],
) -> Response {
    crate::error::problem::written_response(problem, status, described, branches)
}

/// The headers group `H` declares, as one failure's entry in
/// [`declare_headers`]; `()` for a failure sending none.
#[must_use]
pub fn headers<H: HeaderParams>(registry: &mut Registry) -> Map<RefOr<Header>> {
    crate::response::headers::described::<H>(registry)
}

/// Declares on one status's `response` the header groups its failures send,
/// one per failure answering with it, in declaration order.
pub fn declare_headers(response: &mut Response, groups: &[Map<RefOr<Header>>]) {
    crate::response::headers::declare_per_failure(response, groups);
}

/// What a transparent variant answers as: a derived `ApiError` or a refusal
/// Kynos ships, so `IntoProblem` and `ShortCircuit` both.
///
/// The expansion reaches the held error's statuses, problem, response and
/// description through this one bound, so a field holding anything else is
/// reported as no `ApiError`, naming the attribute that made the variant
/// transparent and the status that opts it out, rather than once per trait
/// each use needs under that trait's own wording.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not an `ApiError`, so a transparent variant cannot answer as it",
    label = "not an `ApiError`",
    note = "a variant marked `#[error(transparent)]` or `#[problem(transparent)]` answers with \
            the problem, statuses and responses of the error it holds; derive \
            `kynos::ApiError` for `{Self}`",
    note = "a variant whose message is its field's but whose failure is its own declares \
            `#[problem(status = ...)]`, which makes it no longer transparent"
)]
pub trait HeldError: Sized {
    /// The statuses the held error answers with.
    const STATUSES: &'static [u16];

    /// The held error's problem.
    fn into_problem(self) -> crate::error::problem::Problem;

    /// The held error's response, its headers included.
    fn into_response(self) -> crate::http::Response;

    /// The responses the held error declares.
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses;
}

impl<E: crate::error::problem::IntoProblem + crate::response::ShortCircuit> HeldError for E {
    const STATUSES: &'static [u16] = <E as crate::response::ShortCircuit>::STATUSES;

    #[inline]
    fn into_problem(self) -> crate::error::problem::Problem {
        crate::error::problem::IntoProblem::into_problem(self)
    }

    #[inline]
    fn into_response(self) -> crate::http::Response {
        crate::response::IntoResponse::into_response(self)
    }

    #[inline]
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        <E as crate::response::Responses>::responses(registry)
    }
}

/// Joins a transparent variant's `held` responses to the enum's, a status both
/// declare naming every type and header reaching it, after `held` is titled at
/// each status in `written`, whose description the enum writes over the union.
pub fn union_responses(responses: &mut Responses, held: Responses, written: &[u16]) {
    crate::response::headers::union_from(responses, &crate::error::problem::titled(held, written));
}

/// Writes a failure's header group onto the problem response it answers with,
/// as `WithHeaders` writes its own, a group that refuses to encode answering
/// 500 in its place.
pub fn write_headers<H: EncodeHeaders>(response: &mut crate::http::Response, group: &H) {
    crate::response::headers::write_or_refuse(response, group);
}

/// What a `#[problem(retry_after)]` field holds: a delay every occurrence
/// sends, or one sent only when it holds a value.
#[diagnostic::on_unimplemented(
    message = "`#[problem(retry_after)]` marks a `Duration` or an `Option<Duration>`, not \
               `{Self}`",
    label = "not a delay",
    note = "the derive sends the delay in whole seconds rounded up, as \
            `kynos::http::retry_after::RetryAfter` does; a field holding a header group of its \
            own says `#[problem(headers)]`"
)]
pub trait RetryAfterField {
    /// Whether every occurrence sends the field, so its declaration is
    /// required.
    const ALWAYS: bool;

    /// The delay to send, if any.
    fn delay(&self) -> Option<Duration>;
}

impl RetryAfterField for Duration {
    const ALWAYS: bool = true;

    #[inline]
    fn delay(&self) -> Option<Duration> {
        Some(*self)
    }
}

impl RetryAfterField for Option<Duration> {
    const ALWAYS: bool = false;

    #[inline]
    fn delay(&self) -> Option<Duration> {
        *self
    }
}

/// Writes a failure's `#[problem(retry_after)]` field onto the problem
/// response it answers with, as [`RetryAfter`] sends it; nothing where the
/// field holds no delay.
pub fn write_retry_after<F: RetryAfterField>(response: &mut crate::http::Response, field: &F) {
    if let Some(delay) = field.delay() {
        write_headers(response, &RetryAfter::new(delay));
    }
}

/// Adds to one failure's `group`, as [`headers`] read it, the `Retry-After`
/// its `#[problem(retry_after)]` field sends, described as [`RetryAfter`]
/// describes it and required only where every occurrence sends it.
pub fn with_retry_after<F: RetryAfterField>(
    registry: &mut Registry,
    group: &mut Map<RefOr<Header>>,
) {
    for (name, mut header) in headers::<RetryAfter>(registry) {
        match &mut header {
            RefOr::Item(header) if !F::ALWAYS => header.required = None,
            _ => {}
        }
        group.insert(name, header);
    }
}

/// Whether a header group's `names` hold `Retry-After`, which a failure's
/// `#[problem(retry_after)]` field already sends; compared as RFC 9110
/// section 5.1 compares field names, in a `const`.
#[must_use]
pub const fn names_retry_after(names: &[&str]) -> bool {
    const RETRY_AFTER: &[u8] = b"retry-after";

    let mut at = 0;
    while at < names.len() {
        let name = names[at].as_bytes();
        if name.len() == RETRY_AFTER.len() {
            let mut byte = 0;
            while byte < name.len() && name[byte].eq_ignore_ascii_case(&RETRY_AFTER[byte]) {
                byte += 1;
            }
            if byte == name.len() {
                return true;
            }
        }
        at += 1;
    }
    false
}

/// An extension member as its type serializes it, so the wire follows the
/// serde attributes its `Schema` declares it by; `None` where the type
/// refuses, and the derive answers [`unserializable_member`] instead.
#[must_use]
pub fn to_member<T: serde::Serialize + ?Sized>(value: &T) -> Option<serde_json::Value> {
    serde_json::to_value(value).ok()
}

/// A variant's `#[problem(extension(name = value))]` member as its failure
/// sends it: as [`to_member`] serializes it, and `None` where that refuses or
/// yields `null`, which no member is sent as. The derive answers
/// [`unserializable_member`] in either case.
#[must_use]
pub fn to_constant<T: serde::Serialize + ?Sized>(value: &T) -> Option<serde_json::Value> {
    to_member(value).filter(|member| !member.is_null())
}

/// The schema a variant's `#[problem(extension(name = value))]` member is
/// declared by: a `const` of `value` as [`to_constant`] sends it, so the wire
/// and the description serialize it once each, the same way.
///
/// # Panics
///
/// Where `value`'s `Serialize` refuses or yields `null`. The value is a
/// constant, so it does so every time, and the description is refused when it
/// is built rather than declaring a member the failure can never send.
#[must_use]
pub fn constant<T: serde::Serialize + ?Sized>(value: &T) -> OpenApiSchema {
    let value = to_member(value)
        .expect("a constant extension member's value serializes, as its failure must send it");
    assert!(
        !value.is_null(),
        "a constant extension member's value is never null: a member is sent only when it holds \
         a value, so an optional one is an `Option` field rather than a constant holding `None`"
    );
    crate::error::problem::constant(value)
}

/// The 500 a derived problem answers with in place of one whose extension
/// member refused to serialize, as a JSON body that cannot be serialized does.
#[must_use]
pub fn unserializable_member() -> crate::error::problem::Problem {
    crate::error::problem::Problem::new(crate::http::StatusCode::INTERNAL_SERVER_ERROR)
        .with_detail("a problem extension member could not be serialized")
}

/// `base` followed by `slug`, as the bytes of a `const`: how a type URI under
/// a `#[problem(base = PATH)]` prefix is built at compile time.
///
/// # Panics
///
/// Where `N` is not the two lengths' sum, which in a `const` is a compile
/// error.
#[must_use]
pub const fn join<const N: usize>(base: &str, slug: &str) -> [u8; N] {
    assert!(base.len() + slug.len() == N, "`N` is the joined length");
    let (base, slug) = (base.as_bytes(), slug.as_bytes());
    let mut joined = [0; N];
    let at = crate::http::copy_into(&mut joined, 0, base);
    crate::http::copy_into(&mut joined, at, slug);
    joined
}

/// How many distinct statuses `parts` holds: the length [`union`] is asked
/// for.
#[must_use]
pub const fn union_len(parts: &[&[u16]]) -> usize {
    let mut len = 0;
    let mut part = 0;
    while part < parts.len() {
        let mut index = 0;
        while index < parts[part].len() {
            if first_sighting(parts, part, index) {
                len += 1;
            }
            index += 1;
        }
        part += 1;
    }
    len
}

/// Every status `parts` holds, in order of first appearance and without
/// repeats: how a `#[problem(transparent)]` variant's statuses join its
/// enum's in a `const`.
///
/// # Panics
///
/// Where `N` is not [`union_len`] of `parts`, which in a `const` is a compile
/// error.
#[must_use]
pub const fn union<const N: usize>(parts: &[&[u16]]) -> [u16; N] {
    assert!(union_len(parts) == N, "`N` is the union's length");
    let mut union = [0; N];
    let mut len = 0;
    let mut part = 0;
    while part < parts.len() {
        let mut index = 0;
        while index < parts[part].len() {
            if first_sighting(parts, part, index) {
                union[len] = parts[part][index];
                len += 1;
            }
            index += 1;
        }
        part += 1;
    }
    union
}

/// Whether `parts[part][index]` appears nowhere before that position.
const fn first_sighting(parts: &[&[u16]], part: usize, index: usize) -> bool {
    let status = parts[part][index];
    let mut earlier = 0;
    while earlier <= part {
        let end = if earlier == part {
            index
        } else {
            parts[earlier].len()
        };
        let mut seen = 0;
        while seen < end {
            if parts[earlier][seen] == status {
                return false;
            }
            seen += 1;
        }
        earlier += 1;
    }
    true
}

/// A server failure a derived `into_response` is about to consume, held so
/// it can be logged first.
///
/// The expansion calls `(&Failed(&self)).report(status, type_uri)` with
/// [`ReportSource`] and [`ReportDisplay`] both in scope: method resolution
/// takes the first where `E` is an `Error`, so its `source()` chain is logged,
/// and the second, one autoref later, where `E` is only `Display`.
pub struct Failed<'a, E: ?Sized>(pub &'a E);

impl<E: ?Sized> core::fmt::Debug for Failed<'_, E> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.debug_struct("Failed").finish_non_exhaustive()
    }
}

/// Logs a failure with its `source()` chain; see [`Failed`].
pub trait ReportSource {
    /// Emits the event where `trace` is enabled, and nothing otherwise.
    fn report(&self, status: u16, type_uri: &'static str);
}

/// Logs a failure by its `Display` alone; see [`Failed`].
pub trait ReportDisplay {
    /// Emits the event where `trace` is enabled, and nothing otherwise.
    fn report(&self, status: u16, type_uri: &'static str);
}

impl<E: std::error::Error + ?Sized> ReportSource for Failed<'_, E> {
    #[inline]
    fn report(&self, status: u16, type_uri: &'static str) {
        #[cfg(feature = "trace")]
        crate::middleware::trace::reported::emit(status, type_uri, || {
            crate::middleware::trace::reported::Chain::of(self.0)
        });
        #[cfg(not(feature = "trace"))]
        let _ = (status, type_uri);
    }
}

impl<E: core::fmt::Display + ?Sized> ReportDisplay for &Failed<'_, E> {
    #[inline]
    fn report(&self, status: u16, type_uri: &'static str) {
        #[cfg(feature = "trace")]
        crate::middleware::trace::reported::emit(status, type_uri, || {
            crate::middleware::trace::reported::Chain::alone(self.0)
        });
        #[cfg(not(feature = "trace"))]
        let _ = (status, type_uri);
    }
}

/// The `&str` [`join`]'s bytes spell.
///
/// # Panics
///
/// Never on [`join`]'s output, which concatenates two `&str` and so is UTF-8.
#[must_use]
pub const fn utf8(bytes: &'static [u8]) -> &'static str {
    match core::str::from_utf8(bytes) {
        Ok(joined) => joined,
        Err(_) => panic!("two `&str` join into UTF-8"),
    }
}
