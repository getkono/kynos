//! Declaring response headers as part of the return type.

use kynos_openapi::{Header, Map, RefOr};

use crate::{
    error::problem::Problem,
    extract::params::header::{EncodeHeaders, HeaderParams, UnencodableHeader},
    http::{HeaderMap, HeaderName, HeaderValue, Response, StatusCode},
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};

/// A response carrying declared headers alongside its body.
///
/// `H` derives `HeaderParams`, so each header appears in `Response.headers` with
/// own schema.
///
/// The headers are declared on every response `T` describes, so this suits a
/// group every status sends. A header only one failure sends, such as
/// `Retry-After` on a 429, belongs on that failure's field as
/// `#[problem(headers)]`, which `#[derive(ApiError)]` declares under that
/// failure's status alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WithHeaders<T, H> {
    /// The response body.
    pub body: T,
    /// The declared headers.
    pub headers: H,
}

impl<T, H> WithHeaders<T, H> {
    /// Attaches a derived header group to a response body.
    #[must_use]
    pub fn new(body: T, headers: H) -> Self {
        Self { body, headers }
    }
}

/// The body's status is kept; the declared headers are written onto it.
///
/// Written as [`Continued::with_headers`](crate::middleware::Continued::with_headers)
/// writes them: a repeated `Set-Cookie` is appended, `Content-Encoding` replaced.
/// A group that refuses to [`encode`](EncodeHeaders::encode) answers 500 in
/// place of the body's response, rather than sending it without a field the
/// description declares.
impl<T, H> IntoResponse for WithHeaders<T, H>
where
    T: IntoResponse,
    H: EncodeHeaders,
{
    fn into_response(self) -> Response {
        let mut response = self.body.into_response();
        write_or_refuse(&mut response, &self.headers);
        response
    }
}

/// The declared headers join every response the body describes.
///
/// A group whose [`DESCRIBED`](HeaderParams::DESCRIBED) is `false` joins none of
/// them, though it is still checked for conflicts.
impl<T, H> Responses for WithHeaders<T, H>
where
    T: Responses,
    H: HeaderParams,
{
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let mut responses = T::responses(registry);

        if !H::DESCRIBED {
            return responses;
        }

        declare_on_each(&mut responses, &H::response_headers(registry), true);
        responses
    }
}

/// Declares `declared` on every response `responses` describes, the default
/// included; a name a response already declares is replaced only where
/// `overwrite`.
pub(crate) fn declare_on_each(
    responses: &mut kynos_openapi::Responses,
    declared: &Map<RefOr<Header>>,
    overwrite: bool,
) {
    let described = responses
        .default_response
        .iter_mut()
        .chain(responses.responses.values_mut());

    for response in described {
        // A `$ref` names a response the document holds elsewhere, and
        // declaring a field on it would declare it on every other use.
        if let RefOr::Item(response) = response {
            for (name, header) in declared {
                if overwrite {
                    response.headers.insert(name.clone(), header.clone());
                } else {
                    response
                        .headers
                        .entry(name.clone())
                        .or_insert_with(|| header.clone());
                }
            }
        }
    }
}

/// A header whose value is described as `text/plain` constrained by
/// `schema`, as OpenAPI 3.2's Appendix D describes one.
pub(crate) fn text_header(schema: kynos_openapi::Schema) -> Header {
    Header::with_content(
        kynos_openapi::model::body::mime_names::TEXT_PLAIN,
        kynos_openapi::MediaType::new(schema),
    )
}

/// The 500 a writer answers with in place of a response whose header group
/// refused to encode.
pub(crate) fn refused() -> Response {
    Problem::new(StatusCode::INTERNAL_SERVER_ERROR)
        .with_detail("a response header could not be encoded")
        .into_response()
}

/// Writes `group` onto `fields`, honouring [`REPEATABLE`](HeaderParams::REPEATABLE)
/// and merging [`VARIES`](HeaderParams::VARIES).
///
/// The one writer for both `Continued::with_headers` and `WithHeaders`, so the
/// two cannot disagree. A refused group writes nothing, `VARIES` included; the
/// caller answers with [`refused`] instead.
pub(crate) fn write<G: EncodeHeaders>(
    fields: &mut HeaderMap,
    group: &G,
) -> Result<(), UnencodableHeader> {
    write_encoded::<G>(fields, group.encode()?);
    Ok(())
}

/// Writes what group `G` already [`encode`](EncodeHeaders::encode)d onto
/// `fields`, as [`write`] does, for a writer that encoded it once to use twice.
pub(crate) fn write_encoded<G: EncodeHeaders>(
    fields: &mut HeaderMap,
    encoded: Vec<(HeaderName, HeaderValue)>,
) {
    for (name, value) in encoded {
        // Encoded fields must be a subset of `NAMES`, which the conflict check
        // compares. Debug-only: the response path does not panic.
        debug_assert!(
            G::NAMES
                .iter()
                .any(|declared| crate::http::header_name_eq(declared, name.as_str())),
            "`{}` encodes `{}`, which its `NAMES` does not declare",
            std::any::type_name::<G>(),
            name.as_str(),
        );

        if G::REPEATABLE {
            fields.append(name, value);
        } else {
            fields.insert(name, value);
        }
    }

    // `VARIES` names are not in `NAMES`, so they skip the check above.
    crate::http::vary_on(fields, G::VARIES);
}

/// Writes `group` onto `response`, or replaces `response` with [`refused`]
/// where the group refuses to encode.
///
/// For a writer that has nothing left to add once the group is written.
pub(crate) fn write_or_refuse<G: EncodeHeaders>(response: &mut Response, group: &G) {
    if write(response.headers_mut(), group).is_err() {
        *response = refused();
    }
}

/// The headers `H` declares on a response, or none where
/// [`DESCRIBED`](HeaderParams::DESCRIBED) is `false`.
pub(crate) fn described<H: HeaderParams>(registry: &mut Registry) -> Map<RefOr<Header>> {
    if H::DESCRIBED {
        H::response_headers(registry)
    } else {
        Map::new()
    }
}

/// Declares on `response` the headers its failures send, one group per
/// failure answering with its status, an empty one where a failure sends none.
///
/// A header is `required` only where every group requires it; the first group
/// naming it, compared case-insensitively as RFC 9110 section 5.1 compares
/// field names, describes it.
pub(crate) fn declare_per_failure(
    response: &mut kynos_openapi::Response,
    groups: &[Map<RefOr<Header>>],
) {
    for (index, group) in groups.iter().enumerate() {
        for (name, header) in group {
            let names = |other: &Map<RefOr<Header>>| {
                other.keys().any(|other| other.eq_ignore_ascii_case(name))
            };
            if groups[..index].iter().any(names) {
                continue;
            }

            let requires = |other: &Map<RefOr<Header>>| {
                other.iter().any(|(other, header)| {
                    other.eq_ignore_ascii_case(name)
                        && matches!(header, RefOr::Item(header) if header.required == Some(true))
                })
            };
            let mut header = header.clone();
            if let RefOr::Item(header) = &mut header {
                if !groups.iter().all(requires) {
                    header.required = None;
                }
            }
            response.headers.insert(name.clone(), header);
        }
    }
}

/// [`Responses::union_from`](kynos_openapi::Responses::union_from), the
/// headers of a status both sides declare unioned as [`declare_per_failure`]
/// unions one status's groups, rather than the incoming side's dropped.
pub(crate) fn union_from(
    responses: &mut kynos_openapi::Responses,
    held: &kynos_openapi::Responses,
) {
    // Each status both declare, with the headers this side declared on it.
    let shared: Vec<(String, Map<RefOr<Header>>)> = held
        .responses
        .keys()
        .filter_map(|key| match responses.responses.get(key) {
            Some(RefOr::Item(response)) => Some((key.clone(), response.headers.clone())),
            _ => None,
        })
        .collect();

    responses.union_from(held);

    for (key, declared) in shared {
        if let (Some(RefOr::Item(response)), Some(RefOr::Item(incoming))) =
            (responses.responses.get_mut(&key), held.responses.get(&key))
        {
            response.headers.clear();
            declare_per_failure(response, &[declared, incoming.headers.clone()]);
        }
    }
}

#[cfg(test)]
mod tests;
