//! Request tracing: the one standard way to log at the operation level.

use crate::{http, middleware::Observer};

/// Emits one `tracing` span per operation.
///
/// The span is named by `operation_id` and carries `method`,
/// `matched_path`, `operation_id`, `status`, `latency` and `request_id`.
/// Handler bodies use plain `tracing::info!` and inherit it, so there is no
/// per-endpoint logging middleware to attach and nothing to forget.
///
/// `matched_path` is exactly the `paths` key from the description, which
/// makes it the correct metric label: bounded cardinality, and it lines up
/// with the documented operation.
///
/// Choosing a subscriber remains the application's decision — Kynos depends
/// on the `tracing` facade and nothing more.
#[derive(Clone, Debug, Default)]
pub struct Trace {
    _private: (),
}

impl Trace {
    /// Traces every operation at `INFO`.
    #[must_use]
    pub fn new() -> Self {
        todo!()
    }

    /// Sets the level spans are emitted at.
    #[must_use]
    pub fn level(self, level: tracing::Level) -> Self {
        let _ = level;
        todo!()
    }

    /// Records request headers matching these names on the span.
    ///
    /// Anything not listed is omitted, so a header carrying a credential
    /// cannot end up in a log by accident.
    #[must_use]
    pub fn record_headers(self, names: &'static [&'static str]) -> Self {
        let _ = names;
        todo!()
    }
}

impl<C> Observer<C> for Trace {
    fn on_request(&self, request: &http::Request, context: &C) {
        let _ = (request, context);
        todo!()
    }

    fn on_response(&self, response: &http::Response, elapsed: std::time::Duration) {
        let _ = (response, elapsed);
        todo!()
    }

    fn on_panic(&self, payload: &(dyn std::any::Any + Send)) {
        let _ = payload;
        todo!()
    }
}
