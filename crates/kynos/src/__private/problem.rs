//! What `#[derive(ApiError)]` declares each status with, joins a `const` type
//! URI prefix with, unions a transparent variant's statuses with, and sends
//! and declares a failure's `#[problem(headers)]` group with.
//!
//! [`response`] forwards to [`error::problem`](crate::error::problem), where
//! the shape is built, and the header helpers to
//! [`response::headers`](crate::response::headers), where `WithHeaders` writes
//! and declares its own; a `pub use` would give each a second path.

use kynos_openapi::{Header, Map, RefOr, Response, Schema as OpenApiSchema};

use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams},
    schema::registry::Registry,
};

/// The response one status declares, narrowed to the types its failures
/// publish.
///
/// Each branch is the type URI a failure publishes, or `None` where it names
/// none, the summary its declaration gave it, and its extension members: each
/// one's name, its type's schema, and whether every occurrence sends it.
#[must_use]
pub fn response(
    problem: &OpenApiSchema,
    status: u16,
    branches: &[crate::error::problem::Branch<'_>],
) -> Response {
    crate::error::problem::narrowed_response(problem, status, branches)
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

/// Joins a transparent variant's `held` responses to the enum's, a status both
/// declare naming every type and every header that reaches it.
pub fn union_responses(responses: &mut kynos_openapi::Responses, held: &kynos_openapi::Responses) {
    crate::response::headers::union_from(responses, held);
}

/// Writes a failure's header group onto the problem response it answers with,
/// as `WithHeaders` writes its own, a group that refuses to encode answering
/// 500 in its place.
pub fn write_headers<H: EncodeHeaders>(response: &mut crate::http::Response, group: &H) {
    crate::extract::params::header::write_or_refuse(response, group);
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
