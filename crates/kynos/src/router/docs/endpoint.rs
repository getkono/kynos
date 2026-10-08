//! The two operations a reference mounts.
//!
//! Neither is special. Each is an ordinary [`Endpoint`] with one status, one
//! media type and a payload rendered before the service existed -- which is the
//! only thing about them the rest of the router has to know.

use std::sync::Arc;

use kynos_openapi::{Method, PathTemplate, model::body::mime_names};

use crate::{
    http::{HeaderMap, HeaderValue, Request, Response, body::Body, header},
    router::{docs::State, endpoint::Endpoint, operation::OperationCx},
};

/// The charset is part of the constant rather than left to the recipient to
/// sniff, which is the call [`media::Html`](crate::http::media::Html) already
/// makes for the same bytes.
const HTML: &str = "text/html; charset=utf-8";

/// Taken from the ungated `mime_names` rather than from
/// [`media::Json`](crate::http::media::Json), which is behind the `json`
/// feature. That feature is about *application* payloads, and a reference that
/// implied it would tie the page a human opens to a codec it never uses.
const JSON: &str = mime_names::APPLICATION_JSON;

/// The page a human opens.
#[derive(Debug)]
pub(super) struct DocsPage {
    template: PathTemplate,
    operation_id: String,
    state: Arc<State>,
}

/// The description that page fetches.
#[derive(Debug)]
pub(super) struct DocsDescription {
    template: PathTemplate,
    operation_id: String,
    state: Arc<State>,
}

impl DocsPage {
    pub(super) fn new(template: PathTemplate, operation_id: String, state: Arc<State>) -> Self {
        Self {
            template,
            operation_id,
            state,
        }
    }
}

impl DocsDescription {
    pub(super) fn new(template: PathTemplate, operation_id: String, state: Arc<State>) -> Self {
        Self {
            template,
            operation_id,
            state,
        }
    }
}

impl<C: Send + Sync + 'static> Endpoint<C> for DocsPage {
    fn method(&self) -> Method {
        Method::Get
    }

    fn path(&self) -> &PathTemplate {
        &self.template
    }

    fn describe(&self, operation: &mut OperationCx<'_>) {
        operation.set_operation_id(&self.operation_id);
        operation.set_summary("Serves the API reference");
        operation.set_description(
            "The page a human opens. It fetches this API's own description and renders it in \
             the browser, so this operation sends the page and nothing else.",
        );

        operation.add_responses(&kynos_openapi::Responses::new().with(
            200,
            kynos_openapi::Response::with_content(
                "the reference page",
                HTML,
                // The same unconstrained object every non-JSON payload
                // describes: HTML has no JSON Schema, and one claiming
                // otherwise would claim more than it can check.
                kynos_openapi::MediaType::new(kynos_openapi::Schema::Object(Box::default())),
            ),
        ));
    }

    async fn call(&self, request: Request, context: &C) -> Response {
        // The page is the same for every caller: nothing here reads the
        // request, which is why the operation declares no parameter.
        let _ = (request, context);
        let mut response = answer(self.state.page().clone(), HTML);

        if let Some(policy) = self.state.policy() {
            secure(response.headers_mut(), policy);
        }

        response
    }
}

impl<C: Send + Sync + 'static> Endpoint<C> for DocsDescription {
    fn method(&self) -> Method {
        Method::Get
    }

    fn path(&self) -> &PathTemplate {
        &self.template
    }

    fn describe(&self, operation: &mut OperationCx<'_>) {
        operation.set_operation_id(&self.operation_id);
        operation.set_summary("Serves this API's own description");
        operation.set_description(
            "The document `Router::openapi` produces, byte for byte. It includes this \
             operation, which is why it is serialized once the router is built rather than \
             on the way past.",
        );

        operation.add_responses(&kynos_openapi::Responses::new().with(
            200,
            kynos_openapi::Response::with_content(
                "the OpenAPI description",
                JSON,
                // An OpenAPI document has no `Schema` implementation and will
                // not get one: modelling the meta-schema is a thing this
                // framework deliberately does not do.
                kynos_openapi::MediaType::new(kynos_openapi::Schema::Object(Box::default())),
            ),
        ));
    }

    async fn call(&self, request: Request, context: &C) -> Response {
        let _ = (request, context);
        answer(self.state.description().clone(), JSON)
    }
}

/// One payload rendered before the service existed, with the media type it was
/// rendered as.
///
/// Built directly rather than through
/// [`Binary<M>`](crate::extract::body::binary::Binary): the marker for
/// `application/json` is behind the `json` feature, and a reference must not
/// imply an application codec.
fn answer(bytes: bytes::Bytes, media_type: &'static str) -> Response {
    let mut response = Response::new(Body::from_bytes(bytes));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(media_type));
    response
}

/// The headers a shipped page is served with.
///
/// The policy confines script to the pinned bundle and its boot script, and
/// `nosniff` keeps the browser from reading the page as anything but the HTML
/// it is declared as. A custom page gets neither: see [`Docs::custom`].
///
/// [`Docs::custom`]: crate::router::docs::Docs::custom
fn secure(headers: &mut HeaderMap, policy: &'static str) {
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(policy),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
}
