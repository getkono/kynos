//! The body refusals an operation declares, each sent from one request
//! template by [`TestClient::assert_body_rejections`].

use bytes::Bytes;
use kynos_openapi::{Document, Method, Operation, RefOr, RequestBody};
use serde_json::{Value, json};

use crate::{
    http::{
        HeaderValue, Method as HttpMethod, StatusCode, Uri, header,
        media::{is_json, split_essence},
    },
    test::{
        TestClient, TestRequest,
        body::RequestBody as SentBody,
        check_strictly,
        conformance::{
            matched_template, representation::declared_representation, resolve_media_type, validate,
        },
    },
};

/// Sends each body refusal the operation `template` targets declares, each
/// request begun by `template`, and asserts each answer; `violating` is the
/// 422's body where the test names one.
pub(super) async fn assert<'c, C>(
    client: &'c TestClient<C>,
    template: impl Fn() -> TestRequest<'c, C>,
    violating: Option<Value>,
) {
    let first = template();
    assert!(
        std::ptr::eq(first.client, client),
        "{} {}: the template begins its request on another client than the one \
             asserting its refusals",
        first.method,
        first.path
    );
    let (method, path) = (first.method.clone(), first.path.clone());
    drop(first);

    let uri = path
        .parse::<Uri>()
        .unwrap_or_else(|_| panic!("`{path}` is not a request target"));
    let refusals = plan(client.openapi(), &method, &uri, violating.as_ref())
        .unwrap_or_else(|reason| panic!("{method} {path}: {reason}"));

    for refusal in refusals {
        let status = refusal.status();
        let request = template();
        assert!(
            request.method == method && request.path == path,
            "{method} {path}: the template began {} {} on a later call; it must begin \
                 the same request each time",
            request.method,
            request.path
        );
        let (response, at) = refusal.applied_to(request).send_filed().await;
        response.assert_untyped_problem(status);
        check_strictly(Some(client.openapi()), &client.observed, at);
    }
}

/// One request [`plan`] sends, by the refusal it is sent for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Refusal {
    /// 415: `media_type`, which no key covers, or with `coded` a declared
    /// one under a content coding nothing decodes.
    UnsupportedMediaType { media_type: String, coded: bool },
    /// 400: a body that is not JSON.
    Malformed { media_type: String },
    /// 413: a body declaring no length and never ending.
    TooLarge { media_type: String },
    /// 408: a body that never arrives.
    #[cfg(feature = "server")]
    Stalled { media_type: String },
    /// 422: well-formed JSON the declared schema refuses.
    Violating { media_type: String, body: Bytes },
}

/// What a 415 is sent as where no `requestBody` key covers it.
const UNDECLARED_MEDIA_TYPES: [&str; 2] =
    ["application/x-kynos-undeclared", "text/x-kynos-undeclared"];

/// The content coding a 415 is sent under where `*/*` covers every media
/// type: registered nowhere, so nothing decodes it.
const UNDECLARED_CODING: &str = "x-kynos-undeclared";

/// Malformed from its first octet, so no decoder reads a value first and
/// refuses its type, a 422, rather than its syntax.
const MALFORMED_JSON: &[u8] = b"}";

/// The 422's candidates, in the order the first the schema refuses is taken.
fn violating_candidates() -> [Value; 6] {
    [
        json!(true),
        json!("kynos"),
        json!(0),
        json!([]),
        json!({}),
        Value::Null,
    ]
}

impl Refusal {
    /// The status this request is answered with.
    pub(super) fn status(&self) -> StatusCode {
        match self {
            Self::UnsupportedMediaType { .. } => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::Malformed { .. } => StatusCode::BAD_REQUEST,
            Self::TooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            #[cfg(feature = "server")]
            Self::Stalled { .. } => StatusCode::REQUEST_TIMEOUT,
            Self::Violating { .. } => StatusCode::UNPROCESSABLE_ENTITY,
        }
    }

    /// `request` with its body, and the fields framing it, replaced by this
    /// refusal's.
    fn applied_to<C>(self, mut request: TestRequest<'_, C>) -> TestRequest<'_, C> {
        request.content_length = None;
        for name in [
            header::CONTENT_LENGTH,
            header::CONTENT_TYPE,
            header::CONTENT_ENCODING,
        ] {
            request.headers.remove(name);
        }

        match self {
            Self::UnsupportedMediaType { media_type, coded } => {
                if coded {
                    request.headers.insert(
                        header::CONTENT_ENCODING,
                        HeaderValue::from_static(UNDECLARED_CODING),
                    );
                }
                request.body(&media_type, Bytes::from_static(b"{}"))
            }
            Self::Malformed { media_type } => {
                request.body(&media_type, Bytes::from_static(MALFORMED_JSON))
            }
            Self::TooLarge { media_type } => {
                request.body = SentBody::Endless;
                request.content_type(&media_type)
            }
            #[cfg(feature = "server")]
            Self::Stalled { media_type } => request.stalled_body(&media_type),
            Self::Violating { media_type, body } => request.body(&media_type, body),
        }
    }
}

/// The requests that produce each body refusal the operation `method` and
/// `uri` reach in `document` declares, in the order they are sent; or why
/// there are none to send.
pub(super) fn plan(
    document: &Document,
    method: &HttpMethod,
    uri: &Uri,
    violating: Option<&Value>,
) -> Result<Vec<Refusal>, String> {
    let (template, operation, request_body) = reading(document, method, uri)?;
    let content = &request_body.content;
    let declares = |status: u16| {
        operation
            .responses
            .responses
            .contains_key(&status.to_string())
    };

    let json = content
        .keys()
        .find(|key| is_sendable_json(key))
        .map(|key| sendable(key));
    let media_type = json
        .clone()
        .or_else(|| content.keys().next().map(|key| sendable(key)))
        .ok_or_else(|| {
            format!("the `requestBody` of `{method} {template}` declares no media type")
        })?;

    let mut refusals = Vec::new();

    if declares(415) {
        refusals.push(
            match UNDECLARED_MEDIA_TYPES
                .into_iter()
                .find(|candidate| declared_representation(content, candidate).is_none())
            {
                Some(undeclared) => Refusal::UnsupportedMediaType {
                    media_type: undeclared.to_owned(),
                    coded: false,
                },
                None => Refusal::UnsupportedMediaType {
                    media_type: media_type.clone(),
                    coded: true,
                },
            },
        );
    }
    if let (true, Some(json)) = (declares(400), &json) {
        refusals.push(Refusal::Malformed {
            media_type: json.clone(),
        });
    }
    if declares(413) {
        refusals.push(Refusal::TooLarge {
            media_type: media_type.clone(),
        });
    }
    #[cfg(feature = "server")]
    if declares(408) {
        refusals.push(Refusal::Stalled {
            media_type: media_type.clone(),
        });
    }

    match (declares(422), &json, violating) {
        (true, Some(json), violating) => {
            let body = match violating {
                Some(body) => body.clone(),
                None => refused_candidate(document, content, json).ok_or_else(|| {
                    format!(
                        "the `{json}` schema of `{method} {template}` accepts every candidate \
                         422 body; name one with `assert_body_rejections_violating`"
                    )
                })?,
            };
            refusals.push(Refusal::Violating {
                media_type: json.clone(),
                body: Bytes::from(serde_json::to_vec(&body).expect("a JSON value serializes")),
            });
        }
        (false, _, Some(_)) => {
            return Err(format!(
                "a 422 body was named, but `{method} {template}` declares no 422"
            ));
        }
        (true, None, Some(_)) => {
            return Err(format!(
                "a 422 body was named, but `{method} {template}` reads no JSON body"
            ));
        }
        (_, _, None) => {}
    }

    if refusals.is_empty() {
        return Err(format!(
            "`{method} {template}` declares none of the body refusals a test can send it"
        ));
    }
    Ok(refusals)
}

/// The template, the operation and the `requestBody` it reads, for the
/// operation `method` and `uri` reach in `document`; or why there is none.
fn reading<'d>(
    document: &'d Document,
    method: &HttpMethod,
    uri: &Uri,
) -> Result<(&'d str, &'d Operation, &'d RequestBody), String> {
    let template = matched_template(document, uri)
        .ok_or_else(|| "no declared path matches this request".to_owned())?;
    let operation = Method::from_wire_str(method.as_str())
        .and_then(|method| document.paths.items.get(template)?.operation(method))
        .ok_or_else(|| format!("`{template}` declares no `{method}` operation"))?;
    let request_body = match operation.request_body.as_ref() {
        None => return Err(format!("`{method} {template}` declares no `requestBody`")),
        Some(RefOr::Item(request_body)) => request_body,
        Some(RefOr::Ref(reference)) => reference
            .location
            .strip_prefix("#/components/requestBodies/")
            .and_then(|name| document.components.request_bodies.get(name))
            .and_then(RefOr::as_item)
            .ok_or_else(|| {
                format!("the `requestBody` of `{method} {template}` resolves to no component")
            })?,
    };
    Ok((template, operation, request_body))
}

/// The first candidate the schema declared for `json` refuses.
fn refused_candidate(
    document: &Document,
    content: &kynos_openapi::Map<RefOr<kynos_openapi::MediaType>>,
    json: &str,
) -> Option<Value> {
    let (_, entry) = declared_representation(content, &essence(json))?;
    let schema = resolve_media_type(document, entry)?.schema.as_ref()?;
    violating_candidates()
        .into_iter()
        .find(|candidate| !validate(document, schema, candidate).is_empty())
}

/// A `content` key's media type, lowercased and without its parameters.
fn essence(key: &str) -> String {
    std::str::from_utf8(split_essence(key.as_bytes()).0)
        .expect("a `str` split at an ASCII byte")
        .to_ascii_lowercase()
}

/// Whether `key` names a JSON media type a request can carry as written:
/// `application/json` or a `+json` type, and no range.
fn is_sendable_json(key: &str) -> bool {
    is_json(key) && !essence(key).contains('*')
}

/// A media type a request can carry under `key`: the key itself, a concrete
/// type within it where it is a range, and with a `boundary` where it is a
/// multipart type naming none, without which no multipart body is read.
pub(super) fn sendable(key: &str) -> String {
    let essence = essence(key);
    let media_type = match essence.split_once('/') {
        Some(("*", "*")) => "application/octet-stream".to_owned(),
        Some((top, "*")) => format!("{top}/x-kynos"),
        _ => key.trim().to_owned(),
    };
    if essence.starts_with("multipart/") && !media_type.to_ascii_lowercase().contains("boundary=") {
        return format!("{media_type}; boundary=kynos");
    }
    media_type
}
