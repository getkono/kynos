//! A built router, ready to serve.

use std::{pin::Pin, sync::Arc};

use kynos_openapi::Document;

/// A built router, ready to serve.
pub struct Service<C> {
    document: Document,
    handler: Arc<dyn ErasedService>,
    _context: std::marker::PhantomData<fn() -> C>,
}

trait ErasedService: Send + Sync {
    fn call(
        &self,
        request: crate::http::Request,
    ) -> Pin<Box<dyn Future<Output = crate::http::Response> + Send + '_>>;
}

impl<C> std::fmt::Debug for Service<C> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Service").finish_non_exhaustive()
    }
}

impl<C> Service<C> {
    /// The description of the API this service implements.
    #[must_use]
    pub fn openapi(&self) -> &Document {
        &self.document
    }

    #[cfg(feature = "tls")]
    pub(crate) fn openapi_mut(&mut self) -> &mut Document {
        &mut self.document
    }

    /// Handles one request.
    ///
    /// Exposed so that a Kynos service can be driven directly — by a test, or
    /// by an embedding that owns its own accept loop.
    pub async fn call(&self, request: crate::http::Request) -> crate::http::Response {
        self.handler.call(request).await
    }

    /// Wraps an erased dispatcher and the description it implements.
    ///
    /// Called by [`Router::build`](crate::Router::build). The closure owns the
    /// context and the matcher, which is why a `Service<C>` is `Send + Sync`
    /// whatever `C` is — the context is captured here once rather than being
    /// threaded through every request.
    // `Router::build`, its only caller, is still `todo!()`.
    #[allow(dead_code)]
    pub(crate) fn new<F, Fut>(document: Document, handler: F) -> Self
    where
        F: Fn(crate::http::Request) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = crate::http::Response> + Send + 'static,
    {
        struct Dispatch<F>(F);

        impl<F, Fut> ErasedService for Dispatch<F>
        where
            F: Fn(crate::http::Request) -> Fut + Send + Sync + 'static,
            Fut: Future<Output = crate::http::Response> + Send + 'static,
        {
            fn call(
                &self,
                request: crate::http::Request,
            ) -> Pin<Box<dyn Future<Output = crate::http::Response> + Send + '_>> {
                Box::pin((self.0)(request))
            }
        }

        Self {
            document,
            handler: Arc::new(Dispatch(handler)),
            _context: std::marker::PhantomData,
        }
    }
}
