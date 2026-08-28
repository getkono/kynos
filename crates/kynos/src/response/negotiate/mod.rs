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
/// struct Report {
///     month: String,
/// }
///
/// async fn report(
///     accept: Accept<(Text, Binary<Pdf>)>,
/// ) -> Result<Negotiated<(Text, Binary<Pdf>)>, NegotiationRejection> {
///     let report = Report { month: "2026-08".to_owned() };
///
///     // Closures, not values: the arm the client did not ask for never runs.
///     accept.respond_with(
///         &report,
///         (
///             |report: &Report| Text(report.month.clone()),
///             |report: &Report| Binary::new(render_pdf(report)),
///         ),
///     )
/// }
///
/// fn render_pdf(report: &Report) -> Vec<u8> {
///     report.month.as_bytes().to_vec()
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
    ///
    /// `producers` is a tuple of closures, one per alternative and in the same
    /// order, each handed `source`. Exactly one runs: an alternative the client
    /// did not ask for is never built, so rendering a PDF for a request that
    /// wanted JSON is work that does not happen rather than work that is thrown
    /// away.
    ///
    /// Passing the source separately is what lets every closure see it. Three
    /// closures cannot each own one value, and making them borrow a captured
    /// one would put the same lifetime problem in every handler.
    ///
    /// # Errors
    ///
    /// Returns [`NegotiationRejection::NotAcceptable`] when nothing on offer
    /// matches what the client asked for.
    pub fn respond_with<S, P>(
        self,
        source: &S,
        producers: P,
    ) -> Result<Negotiated<T>, NegotiationRejection>
    where
        T: representation::Representations,
        P: representation::Producers<S, T>,
    {
        let selected = self.choose::<T>()?;

        Ok(Negotiated {
            response: producers.produce_at(source, selected),
            offer: std::marker::PhantomData,
        })
    }

    /// The index of the best alternative, or the 406.
    ///
    /// Split from `respond_with` so the ranking can be asserted without
    /// producing a response: what is worth testing is which arm wins, and
    /// building one would drag in every codec's writer.
    pub(crate) fn choose<O: representation::Representations>(
        &self,
    ) -> Result<usize, NegotiationRejection> {
        O::media_types()
            .iter()
            .enumerate()
            .filter_map(|(index, media_type)| self.score(media_type).map(|score| (score, index)))
            .max_by(|(left_score, left_index), (right_score, right_index)| {
                left_score
                    .cmp(right_score)
                    .then_with(|| right_index.cmp(left_index))
            })
            .map(|(_, index)| index)
            .ok_or(NegotiationRejection::NotAcceptable)
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

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        let mut values = parts.headers.get_all(crate::http::header::ACCEPT).iter();

        // RFC 9110 12.5.1: a request with no `Accept` accepts any media type.
        // A field that is *present* and empty is a different claim, and reaches
        // `parse` so that it is answered as the malformed value it is.
        let Some(first) = values.next() else {
            return Self::parse("*/*");
        };

        // A field that may appear more than once is equivalent to one field
        // holding the comma-separated list, which is the form `parse` reads.
        let mut field = String::new();
        for value in std::iter::once(first).chain(values) {
            let value = value.to_str().map_err(|_| invalid_accept())?;
            if !field.is_empty() {
                field.push(',');
            }
            field.push_str(value);
        }

        Self::parse(&field)
    }
}

impl<T> Describe for Accept<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let responses = NegotiationRejection::responses(operation.registry());
        operation.add_responses(&responses);
    }
}

/// A response whose representation was chosen from the client's `Accept`
/// header.
///
/// `T` is a tuple of response types, each contributing one entry to the
/// operation's `content` map. Note that `Accept` itself is never declared as a
/// parameter — the specification says such a declaration is ignored, and the
/// `content` map is what actually describes the negotiation.
// A response is neither `Clone` nor `PartialEq` -- a body is a stream, not a
// value -- so `Negotiated` cannot be either now that it holds one rather than
// the alternatives it might have built.
#[derive(Debug)]
pub struct Negotiated<T> {
    response: Response,

    /// The offer, kept at the type level.
    ///
    /// `Responses` reads it and nothing else does: the chosen representation is
    /// already a response by the time this exists, and the alternatives were
    /// never built. Keeping `T` is what stops the description losing an arm the
    /// handler could have served.
    offer: std::marker::PhantomData<fn() -> T>,
}

impl<T: representation::Representations> IntoResponse for Negotiated<T> {
    fn into_response(self) -> Response {
        self.response
    }
}

impl<T: representation::Representations> Responses for Negotiated<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        T::responses(registry)
    }
}

#[cfg(test)]
mod tests;
