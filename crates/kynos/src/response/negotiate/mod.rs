//! Choosing a response representation from the client's `Accept` header.
//!
//! Note that `Accept` is never declared as a parameter — the specification says
//! such a declaration is ignored — so what describes the negotiation is the
//! operation's `content` map, contributed by the representation tuple.

pub mod representation;

use crate::{
    error::rejection::NegotiationRejection,
    extract::{FromRequestParts, describe::Describe},
    http::{Parts, Response},
    response::{IntoResponse, Responses},
    router::operation::OperationCx,
    schema::registry::Registry,
};

/// The client's accepted response representations.
///
/// This extractor contributes no `Accept` parameter because OpenAPI ignores
/// such parameters. It contributes the 406 rejection and the representation
/// tuple contributes the operation's response `content` map.
///
/// ```no_run
/// use kynos::{
///     error::rejection::NegotiationRejection,
///     extract::{
///         body::{binary::Binary, text::Text},
///         media::Pdf,
///     },
///     response::negotiate::{Accept, Negotiated},
/// };
///
/// async fn report(
///     accept: Accept<(Text, Binary<Pdf>)>,
/// ) -> Result<Negotiated<(Text, Binary<Pdf>)>, NegotiationRejection> {
///     accept.respond((
///         Text("plain report".to_owned()),
///         Binary::new(Vec::<u8>::new()),
///     ))
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accept<T> {
    preferences: Vec<Preference>,
    representations: std::marker::PhantomData<fn() -> T>,
}

impl<T> Accept<T> {
    /// Parses an `Accept` field value for tests and non-server integrations.
    ///
    /// An absent field is represented by `"*/*"`. Invalid quality values are
    /// rejected as malformed headers.
    pub fn parse(value: &str) -> Result<Self, NegotiationRejection> {
        let mut preferences = Vec::new();
        for (order, item) in value.split(',').enumerate() {
            let mut segments = item.trim().split(';');
            let range = segments.next().unwrap_or_default().trim();
            let Some((type_, subtype)) = range.split_once('/') else {
                return Err(invalid_accept());
            };
            if type_.is_empty() || subtype.is_empty() || (type_ == "*" && subtype != "*") {
                return Err(invalid_accept());
            }

            let mut quality = 1_000;
            for parameter in segments {
                let Some((name, value)) = parameter.trim().split_once('=') else {
                    return Err(invalid_accept());
                };
                if name.trim().eq_ignore_ascii_case("q") {
                    quality = parse_quality(value.trim()).ok_or_else(invalid_accept)?;
                }
            }
            preferences.push(Preference {
                type_: type_.to_ascii_lowercase(),
                subtype: subtype.to_ascii_lowercase(),
                quality,
                order,
            });
        }
        if preferences.is_empty() {
            return Err(invalid_accept());
        }
        Ok(Self {
            preferences,
            representations: std::marker::PhantomData,
        })
    }

    /// Chooses one offered representation or returns a documented 406.
    pub fn respond(self, representations: T) -> Result<Negotiated<T>, NegotiationRejection>
    where
        T: representation::Representations,
    {
        let selected = T::media_types()
            .iter()
            .enumerate()
            .filter_map(|(index, media_type)| self.score(media_type).map(|score| (score, index)))
            .max_by(|(left_score, left_index), (right_score, right_index)| {
                left_score
                    .cmp(right_score)
                    .then_with(|| right_index.cmp(left_index))
            })
            .map(|(_, index)| index)
            .ok_or(NegotiationRejection::NotAcceptable)?;

        Ok(Negotiated {
            representations,
            selected,
        })
    }

    fn score(&self, media_type: &str) -> Option<(u16, u8, std::cmp::Reverse<usize>)> {
        let (type_, subtype) = media_type.split_once('/')?;
        self.preferences
            .iter()
            .filter_map(|preference| {
                let specificity = if preference.type_ == "*" && preference.subtype == "*" {
                    0
                } else if preference.type_.eq_ignore_ascii_case(type_) && preference.subtype == "*"
                {
                    1
                } else if preference.type_.eq_ignore_ascii_case(type_)
                    && preference.subtype.eq_ignore_ascii_case(subtype)
                {
                    2
                } else {
                    return None;
                };
                Some((
                    specificity,
                    std::cmp::Reverse(preference.order),
                    preference.quality,
                ))
            })
            .max_by_key(|(specificity, order, _)| (*specificity, *order))
            .and_then(|(specificity, order, quality)| {
                (quality != 0).then_some((quality, specificity, order))
            })
    }
}

fn parse_quality(value: &str) -> Option<u16> {
    if value == "0" || value == "0.0" || value == "0.00" || value == "0.000" {
        return Some(0);
    }
    if value == "1" || value == "1.0" || value == "1.00" || value == "1.000" {
        return Some(1_000);
    }
    let digits = value.strip_prefix("0.")?;
    if digits.is_empty() || digits.len() > 3 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits
        .parse::<u16>()
        .ok()
        .map(|quality| match digits.len() {
            1 => quality * 100,
            2 => quality * 10,
            _ => quality,
        })
}

fn invalid_accept() -> NegotiationRejection {
    NegotiationRejection::MalformedAccept {
        detail: "expected comma-separated media ranges with q values from 0 to 1".to_owned(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Preference {
    type_: String,
    subtype: String,
    quality: u16,
    order: usize,
}

impl<C: Sync, T: Send> FromRequestParts<C> for Accept<T> {
    type Rejection = NegotiationRejection;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (parts, context);
        todo!()
    }
}

impl<T> Describe for Accept<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let responses = NegotiationRejection::responses(operation.registry());
        operation.add_responses(responses);
    }
}

/// A response whose representation was chosen from the client's `Accept`
/// header.
///
/// `T` is a tuple of response types, each contributing one entry to the
/// operation's `content` map. Note that `Accept` itself is never declared as a
/// parameter — the specification says such a declaration is ignored, and the
/// `content` map is what actually describes the negotiation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Negotiated<T> {
    representations: T,
    selected: usize,
}

impl<T: representation::Representations> IntoResponse for Negotiated<T> {
    fn into_response(self) -> Response {
        self.representations.into_response_at(self.selected)
    }
}

impl<T: representation::Representations> Responses for Negotiated<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        T::responses(registry)
    }
}

#[cfg(test)]
mod tests;
