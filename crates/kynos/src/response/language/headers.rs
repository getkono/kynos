//! The two fields language negotiation reads and writes.
//!
//! # The grammar
//!
//! RFC 9110 sections 12.5.4 and 8.5:
//!
//! ```text
//! Accept-Language  = #( language-range [ weight ] )
//! language-range   = <language-range, see [RFC4647], Section 2.1>
//!
//! Content-Language = #language-tag
//! language-tag     = <Language-Tag, see [RFC5646], Section 2.1>
//! ```
//!
//! # Why `Accept-Language` is a parameter where `Accept` is not
//!
//! OpenAPI names exactly three header fields whose parameter definition "SHALL
//! be ignored" — `Accept`, `Content-Type` and `Authorization` — and
//! `Accept-Language` is not among them. So declaring it is a claim a consumer
//! will honour, where declaring `Accept` is not, and
//! [`negotiate`](crate::response::negotiate) is right to contribute none while
//! this module is right to contribute one.

use kynos_openapi::Parameter;

/// The field a client states its language preferences in.
///
/// The schema is an unconstrained string, and both halves of that are
/// deliberate.
///
/// No `enum`, because the value is a *priority list* rather than a tag:
/// `da, en-gb;q=0.8, en;q=0.7` is RFC 9110's own example and is not a member of
/// any set of offered tags. The offered set is stated on `Content-Language`,
/// where it is true. What the offer does reach here is the description and one
/// example, which is where prose belongs.
///
/// No `pattern` either, unlike [`range::parameter`](crate::response::range::parameter).
/// That field is refused when it is malformed, so a pattern documents a real
/// rejection; this one never is — an unreadable range is dropped and the rest
/// of the field still counts — so a pattern would document a refusal the
/// service does not make.
#[must_use]
pub fn parameter(tags: &[&str]) -> Parameter {
    Parameter::header(
        "Accept-Language",
        kynos_openapi::Schema::of_type(kynos_openapi::model::schema::types::SchemaType::String),
    )
    .with_description(format!(
        "The natural languages preferred in the response, per RFC 9110 section 12.5.4. A \
         comma-separated priority list of RFC 4647 language ranges, each optionally weighted \
         with `;q=`. This operation answers in {}, and states which on `Content-Language`. A \
         request whose preferences match none of them is served {} rather than refused, and a \
         range this field cannot parse is ignored rather than refusing the request.",
        english_list(tags),
        tags.first().unwrap_or(&"the first of them"),
    ))
    .with_example("da, en-gb;q=0.8, en;q=0.7")
}

/// `en`, `fr` and `de`, for a sentence rather than a schema.
fn english_list(tags: &[&str]) -> String {
    match tags {
        [] => "no language in particular".to_owned(),
        [only] => format!("`{only}`"),
        [head @ .., last] => format!(
            "{} and `{last}`",
            head.iter()
                .map(|tag| format!("`{tag}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
