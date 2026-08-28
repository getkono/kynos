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

    /// Flags every operation in the document as unverified, and restamps the
    /// document-level summary to match.
    ///
    /// Used by the escape hatches: the operations stay in `paths`, because an
    /// omission is invisible to the consumer that trusts the description.
    #[cfg(feature = "unchecked")]
    pub(crate) fn mark_opaque(&mut self, reason: kynos_openapi::OpaqueReason) {
        let marker = kynos_openapi::Opaque::new(reason);

        for item in self.document.paths.items.values_mut() {
            let slots: Vec<&mut Option<Box<kynos_openapi::Operation>>> = vec![
                &mut item.get,
                &mut item.put,
                &mut item.post,
                &mut item.delete,
                &mut item.options,
                &mut item.head,
                &mut item.patch,
                &mut item.trace,
                #[cfg(feature = "openapi32")]
                &mut item.query,
            ];

            for slot in slots {
                if let Some(operation) = slot.as_deref_mut() {
                    // The only reachable failure is a marker already present in
                    // a shape Kynos never emits, which a document Kynos just
                    // built cannot carry.
                    let _ = marker.apply_to(operation);
                }
            }

            #[cfg(feature = "openapi32")]
            for operation in item.additional_operations.values_mut() {
                let _ = marker.apply_to(operation);
            }
        }

        // Derived rather than set: the stamp is a summary of what the document
        // now says, in both directions.
        self.document.restamp_authority();
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
