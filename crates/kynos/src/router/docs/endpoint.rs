//! The two operations a reference mounts, each an ordinary [`Endpoint`] with
//! one status, one media type and a payload rendered from the finished document.

use std::sync::Arc;

use kynos_openapi::{Method, PathTemplate, model::body::mime_names};

use crate::{
    http::{HeaderMap, HeaderValue, Request, Response, body::Body, header},
    router::{docs::State, endpoint::Endpoint, operation::OperationCx},
};

/// HTML with an explicit charset, as [`media::Html`](crate::http::media::Html)
/// sends.
const HTML: &str = "text/html; charset=utf-8";

/// From the ungated `mime_names`, so a reference does not require the `json`
/// feature.
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
                // Unconstrained, as every non-JSON payload is described.
                kynos_openapi::MediaType::new(kynos_openapi::Schema::Object(Box::default())),
            ),
        ));
    }

    async fn call(&self, request: Request, context: &C) -> Response {
        // Nothing reads the request, so no parameter is declared.
        let _ = (request, context);
        let mut response = answer(self.state.page(), HTML);

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
                // Unconstrained: the OpenAPI meta-schema is deliberately not modelled.
                kynos_openapi::MediaType::new(kynos_openapi::Schema::Object(Box::default())),
            ),
        ));
    }

    async fn call(&self, request: Request, context: &C) -> Response {
        let _ = (request, context);
        answer(self.state.description(), JSON)
    }
}

/// One payload rendered from the finished document, with the media type it was
/// rendered as. Built directly, since `Binary<Json>` needs the `json` feature.
fn answer(bytes: bytes::Bytes, media_type: &'static str) -> Response {
    let mut response = Response::new(Body::from_bytes(bytes));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(media_type));
    response
}

/// The headers a shipped page is served with: its script policy and `nosniff`.
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
