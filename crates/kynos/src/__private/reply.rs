//! What `#[derive(Reply)]` writes a variant with.
//!
//! Functions rather than emitted tokens, since the deriving crate need not
//! depend on `serde_json`.

use kynos_openapi::{RefOr, StatusPattern, model::body::mime_names};

use crate::{
    error::problem::Problem,
    http::{HeaderValue, Response, StatusCode, body::Body, header},
    response::{IntoResponse, Responses, codec::json_bytes, status},
    schema::registry::Registry,
};

/// The status a variant declared, which the derive checked is one.
fn status_code(status: u16) -> StatusCode {
    StatusCode::from_u16(status).expect("`#[derive(Reply)]` rejects a status outside 200..=599")
}

/// A variant carrying no body: the declared status and nothing else.
#[must_use]
pub fn empty(status: u16) -> Response {
    crate::http::body::bodiless(status_code(status))
}

/// A variant carrying a body, as the `application/json` the derive described it
/// as.
///
/// Serialized in full before anything is written, so a failure is the
/// documented RFC 9457 500 rather than a truncated success.
#[must_use]
pub fn json<T: serde::Serialize>(status: u16, body: &T) -> Response {
    let Ok(bytes) = json_bytes(body) else {
        return Problem::new(StatusCode::INTERNAL_SERVER_ERROR)
            .with_detail("the response body could not be serialized")
            .into_response();
    };

    let mut response = Response::new(Body::from_bytes(bytes));
    *response.status_mut() = status_code(status);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(mime_names::APPLICATION_JSON),
    );
    response
}

/// A variant holding a response: what it writes, under the declared status.
///
/// A 4xx or 5xx it answers with is sent unchanged, as
/// [`Created`](crate::response::status::Created) sends one.
#[must_use]
pub fn held<T: IntoResponse>(status: u16, held: T) -> Response {
    let mut response = held.into_response();
    if !status::is_failure(&response) {
        *response.status_mut() = status_code(status);
    }
    response
}

/// Describes a variant holding `T` under its declared `status`, into
/// `responses`, and collects the 4xx and 5xx `T` passes through into
/// `failures`, which the derive joins after every variant's own status.
///
/// The entry is `T`'s own at `status`, else `T`'s 200 or sole representation
/// re-keyed, as `Created` re-keys its body's. `description` replaces the
/// entry's description. Without it, an entry `T` declares at `status` keeps
/// its own, else takes `fallback`; a re-keyed entry always takes `fallback`,
/// since its 200's description describes a 200. A `$ref` entry is kept as is.
pub fn describe<T: Responses>(
    registry: &mut Registry,
    responses: &mut kynos_openapi::Responses,
    failures: &mut kynos_openapi::Responses,
    status: u16,
    description: Option<&str>,
    fallback: &str,
) {
    let mut described = T::responses(registry);
    let key = StatusPattern::Code(status).to_string();

    let entry = match status::body_response(description.unwrap_or(fallback), status, &mut described)
    {
        Some(mut entry) => {
            if let Some(description) = description {
                entry.description = Some(description.to_owned());
            } else if entry.description.is_none() {
                entry.description = Some(fallback.to_owned());
            }
            RefOr::Item(entry)
        }
        // `body_response` declines only a `$ref` under `status`.
        None => described
            .responses
            .get(&key)
            .cloned()
            .expect("a declined entry is a `$ref` under the status"),
    };
    responses.responses.insert(key.clone(), entry);

    let mut passed = kynos_openapi::Responses::new();
    for (other, response) in &described.responses {
        if *other != key && other.starts_with(['4', '5']) {
            passed.responses.insert(other.clone(), response.clone());
        }
    }
    failures.union_from(&passed);
}

/// Joins the failures [`describe`] collected after every variant's own
/// status, so a status both describe documents everything sent at it.
///
/// Two problem entries meeting on one status are unioned as
/// [`union_from`](kynos_openapi::Responses::union_from) unions them. Where
/// the variant's entry still lacks a representation or header the failure
/// declares (a unit variant's empty 404 beside a held type's problem, say),
/// it gains them, and the failure's description is appended after a `"; "`,
/// less any sentence the entry's already holds.
/// A `$ref` entry on either side is kept as is.
pub fn join(responses: &mut kynos_openapi::Responses, failures: &kynos_openapi::Responses) {
    responses.union_from(failures);
    for (key, incoming) in &failures.responses {
        let (Some(RefOr::Item(declared)), RefOr::Item(incoming)) =
            (responses.responses.get_mut(key), incoming)
        else {
            continue;
        };

        let mut gained = false;
        for (media, representation) in &incoming.content {
            if !declared.content.contains_key(media) {
                declared
                    .content
                    .insert(media.clone(), representation.clone());
                gained = true;
            }
        }
        for (name, header) in &incoming.headers {
            if !declared.headers.contains_key(name) {
                declared.headers.insert(name.clone(), header.clone());
                gained = true;
            }
        }

        if gained {
            declared.description = joined(
                declared.description.as_deref(),
                incoming.description.as_deref(),
            );
        }
    }
}

/// Both descriptions, joined with `"; "` and without repeats.
///
/// Split first, since `union_from` may already have joined the failure's into
/// the entry's.
fn joined(own: Option<&str>, appended: Option<&str>) -> Option<String> {
    let mut sentences: Vec<&str> = Vec::new();
    for sentence in own.into_iter().chain(appended).flat_map(|d| d.split("; ")) {
        if !sentences.contains(&sentence) {
            sentences.push(sentence);
        }
    }

    (!sentences.is_empty()).then(|| sentences.join("; "))
}
