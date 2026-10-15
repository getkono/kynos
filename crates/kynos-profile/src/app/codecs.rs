//! The codec group's handlers: `echo-post`'s document in and back out through
//! each other body codec, and the requests its refusals are measured at.
//!
//! Each request body is what the row's own codec writes for `echo-post`'s
//! value, so the octets a row decodes are the ones it would answer with.

use kynos::{
    extract::body::{form::Form, multipart::MultipartForm, protobuf::Protobuf},
    http::{Method, Request, body::Body, header},
    openapi::model::body::mime_names,
    prelude::*,
    response::IntoResponse,
};

use super::{App, Echo, echo_value, octets, post, request};

/// `echo-post`'s document in the shapes a multipart part takes: text, one
/// part per field and one per item of a sequence, as the form codec writes it.
#[derive(Schema, kynos::MultipartForm)]
struct EchoParts {
    id: String,
    message: String,
    values: Vec<String>,
    labels: Vec<String>,
}

impl From<Echo> for EchoParts {
    fn from(echo: Echo) -> Self {
        Self {
            id: echo.id.to_string(),
            message: echo.message,
            values: echo.values.iter().map(u64::to_string).collect(),
            labels: echo.labels,
        }
    }
}

/// `echo-post`'s document as a Protocol Buffers message.
#[derive(Clone, PartialEq, prost::Message, Schema)]
struct EchoMessage {
    #[prost(uint64, tag = "1")]
    id: u64,
    #[prost(string, tag = "2")]
    message: String,
    #[prost(uint64, repeated, tag = "3")]
    values: Vec<u64>,
    #[prost(string, repeated, tag = "4")]
    labels: Vec<String>,
}

impl From<Echo> for EchoMessage {
    fn from(echo: Echo) -> Self {
        Self {
            id: echo.id,
            message: echo.message,
            values: echo.values,
            labels: echo.labels,
        }
    }
}

#[kynos::post("/form/echo")]
async fn form_echo(Form(echo): Form<Echo>) -> Form<Echo> {
    Form(echo)
}

#[kynos::post("/multipart/echo")]
async fn multipart_echo(MultipartForm(echo): MultipartForm<EchoParts>) -> MultipartForm<EchoParts> {
    MultipartForm(echo)
}

#[kynos::post("/protobuf/echo")]
async fn protobuf_echo(Protobuf(echo): Protobuf<EchoMessage>) -> Protobuf<EchoMessage> {
    Protobuf(echo)
}

/// Mounts this group's handlers.
pub(crate) fn mount(router: Router<App>) -> Router<App> {
    router.mount(kynos::routes![form_echo, multipart_echo, protobuf_echo])
}

/// A `POST` to `target` carrying what `encoded` answers: its body, under its
/// `Content-Type`.
fn echoing(target: &str, encoded: impl IntoResponse) -> Request {
    let (head, body) = encoded.into_response().into_parts();
    let mut request = request(Method::POST, target, None, Body::from_bytes(octets(body)));
    let content_type = head
        .headers
        .get(header::CONTENT_TYPE)
        .expect("every codec names its media type")
        .clone();
    request
        .headers_mut()
        .insert(header::CONTENT_TYPE, content_type);
    request
}

/// `form-echo`'s request.
pub(crate) fn form_request() -> Request {
    echoing("/form/echo", Form(echo_value()))
}

/// `multipart-echo`'s request.
pub(crate) fn multipart_request() -> Request {
    echoing(
        "/multipart/echo",
        MultipartForm(EchoParts::from(echo_value())),
    )
}

/// `protobuf-echo`'s request.
pub(crate) fn protobuf_request() -> Request {
    echoing("/protobuf/echo", Protobuf(EchoMessage::from(echo_value())))
}

/// `rejection`'s body: JSON that ends before its first value.
const TRUNCATED: &[u8] = b"{\"id\":";

/// `rejection`'s request.
pub(crate) fn rejection_request() -> Request {
    post(
        "/echo",
        Body::from_bytes(bytes::Bytes::from_static(TRUNCATED)),
    )
}

/// `rejection`'s request, declared as a media type `/echo` does not decode.
pub(crate) fn reject_415_request() -> Request {
    request(
        Method::POST,
        "/echo",
        Some(mime_names::TEXT_PLAIN),
        Body::from_bytes(bytes::Bytes::from_static(TRUNCATED)),
    )
}

/// `rejection`'s request, declaring a length one octet past the limit a body
/// extractor reads under, which is refused from the head alone.
pub(crate) fn reject_413_request() -> Request {
    let mut request = rejection_request();
    request.headers_mut().insert(
        header::CONTENT_LENGTH,
        header::HeaderValue::from(kynos::extract::body::limit::DEFAULT_LIMIT + 1),
    );
    request
}

/// `rejection`'s request, its body a whole JSON document missing every field
/// but the one `rejection`'s ended inside.
pub(crate) fn reject_422_request() -> Request {
    post(
        "/echo",
        Body::from_bytes(bytes::Bytes::from_static(b"{\"id\":42}")),
    )
}
