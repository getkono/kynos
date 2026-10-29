//! The two operations a reference mounts, each an ordinary [`Endpoint`] with
//! one status, one media type and a payload rendered from the finished document.

use std::sync::Arc;

use kynos_openapi::{Method, PathTemplate, model::body::mime_names};

use crate::{
    http::{
        HeaderMap, HeaderValue, Request, Response,
        body::Body,
        header,
        media::{Html, MediaType},
    },
    response::codec::{unconstrained, with_content_type},
    router::{docs::State, endpoint::Endpoint, operation::OperationCx},
};

/// HTML with an explicit charset, as [`Html`] sends.
const HTML: &str = <Html as MediaType>::MEDIA_TYPE;

/// From the ungated `mime_names`, so a reference does not require the `json`
/// feature.
const JSON: &str = mime_names::APPLICATION_JSON;

/// Which of a reference's two operations an endpoint is.
#[derive(Clone, Copy, Debug)]
pub(super) enum Half {
    /// The page a human opens.
    Page,
    /// The description that page fetches.
    Description,
}

/// One of the two operations a reference mounts.
#[derive(Debug)]
pub(super) struct DocsEndpoint {
    half: Half,
    template: PathTemplate,
    operation_id: String,
    state: Arc<State>,
}

impl DocsEndpoint {
    pub(super) fn new(
        half: Half,
        template: PathTemplate,
        operation_id: String,
        state: Arc<State>,
    ) -> Self {
        Self {
            half,
            template,
            operation_id,
            state,
        }
    }
}

impl<C: Send + Sync + 'static> Endpoint<C> for DocsEndpoint {
    fn method(&self) -> Method {
        Method::Get
    }

    fn path(&self) -> &PathTemplate {
        &self.template
    }

    fn describe(&self, operation: &mut OperationCx<'_>) {
        operation.set_operation_id(&self.operation_id);

        // Each payload is unconstrained: the page as every non-JSON payload is
        // described, and the description because the OpenAPI meta-schema is
        // deliberately not modelled.
        let response = match self.half {
            Half::Page => {
                operation.set_summary("Serves the API reference");
                operation.set_description(
                    "The page a human opens. It fetches this API's own description and renders \
                     it in the browser, so this operation sends the page and nothing else.",
                );
                kynos_openapi::Response::with_content("the reference page", HTML, unconstrained())
            }
            Half::Description => {
                operation.set_summary("Serves this API's own description");
                operation.set_description(
                    "The document `Router::openapi` produces, byte for byte. It includes this \
                     operation, which is why it is serialized once the router is built rather \
                     than on the way past.",
                );
                kynos_openapi::Response::with_content(
                    "the OpenAPI description",
                    JSON,
                    unconstrained(),
                )
            }
        };

        operation.add_responses(&kynos_openapi::Responses::new().with(200, response));
    }

    async fn call(&self, request: Request, context: &C) -> Response {
        // Nothing reads the request, so no parameter is declared.
        let _ = (request, context);
        match self.half {
            Half::Page => {
                let mut response = answer(self.state.page(), HTML);
                if let Some(policy) = self.state.policy() {
                    secure(response.headers_mut(), policy);
                }
                response
            }
            Half::Description => answer(self.state.description(), JSON),
        }
    }
}

/// One payload rendered from the finished document, with the media type it was
/// rendered as. Built directly, since `Binary<Json>` needs the `json` feature.
fn answer(bytes: bytes::Bytes, media_type: &'static str) -> Response {
    with_content_type(
        Body::from_bytes(bytes),
        HeaderValue::from_static(media_type),
    )
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
