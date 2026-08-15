//! Request tracing: the one standard way to log at the operation level.

use crate::{http, middleware::Observer, router::operation::Route};

/// Emits a `tracing` event for each end of an operation.
///
/// Both carry `method`, `matched_path` and `operation_id`; the closing one adds
/// `status` and `latency`, and `request_id` rides on whichever end the header is
/// present at. Handler bodies use plain `tracing::info!` and inherit whatever
/// span the application established, so there is no per-endpoint logging
/// middleware to attach and nothing to forget.
///
/// `matched_path` is exactly the `paths` key from the description, which
/// makes it the correct metric label: bounded cardinality, and it lines up
/// with the documented operation.
///
/// Two events rather than one span, because an [`Observer`] is told when a
/// request arrives and when a response leaves and holds nothing in between: a
/// span covering the handler would have to be entered across a suspension point
/// this trait never sees. Nothing is lost from the record — the pair carries
/// what a span would have — and the alternative would be an observer that could
/// affect the exchange, which is the one thing an observer is defined not to do.
///
/// Choosing a subscriber remains the application's decision — Kynos depends
/// on the `tracing` facade and nothing more.
#[derive(Clone, Debug)]
pub struct Trace {
    level: tracing::Level,
    recorded: &'static [&'static str],
}

/// Emits an event at a level chosen at run time.
///
/// `tracing`'s macros bake the level into a `static` callsite, so a level held
/// in a field has to be matched back onto the constant that names it. This is
/// the one place that happens.
macro_rules! emit {
    ($level:expr, $($event:tt)*) => {
        match $level {
            tracing::Level::ERROR => tracing::event!(tracing::Level::ERROR, $($event)*),
            tracing::Level::WARN => tracing::event!(tracing::Level::WARN, $($event)*),
            tracing::Level::INFO => tracing::event!(tracing::Level::INFO, $($event)*),
            tracing::Level::DEBUG => tracing::event!(tracing::Level::DEBUG, $($event)*),
            tracing::Level::TRACE => tracing::event!(tracing::Level::TRACE, $($event)*),
        }
    };
}

/// What an unmatched request has instead of a `paths` key.
///
/// A placeholder rather than an omitted field, so that a log line for a 404 has
/// the same shape as every other one.
const UNMATCHED: &str = "<unmatched>";

impl Trace {
    /// Traces every operation at `INFO`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            level: tracing::Level::INFO,
            recorded: &[],
        }
    }

    /// Sets the level events are emitted at.
    ///
    /// A panic is always reported at `ERROR`, whatever this says.
    #[must_use]
    pub fn level(mut self, level: tracing::Level) -> Self {
        self.level = level;
        self
    }

    /// Records request headers matching these names on the emitted events.
    ///
    /// Anything not listed is omitted, so a header carrying a credential
    /// cannot end up in a log by accident.
    #[must_use]
    pub fn record_headers(mut self, names: &'static [&'static str]) -> Self {
        self.recorded = names;
        self
    }

    /// The listed headers this request carries, as one field value.
    ///
    /// One field rather than one per header: a `tracing` field name is fixed at
    /// its callsite, and the names here are chosen by the application.
    fn recorded(&self, headers: &http::HeaderMap) -> String {
        let mut recorded = String::new();

        for name in self.recorded {
            let Some(value) = headers.get(*name).and_then(|value| value.to_str().ok()) else {
                continue;
            };

            if !recorded.is_empty() {
                recorded.push_str(", ");
            }
            recorded.push_str(name);
            recorded.push('=');
            recorded.push_str(value);
        }

        recorded
    }
}

impl Default for Trace {
    fn default() -> Self {
        Self::new()
    }
}

/// The correlation identifier a request or response carries, if any.
fn request_id(headers: &http::HeaderMap) -> &str {
    headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
}

impl<C> Observer<C> for Trace {
    fn on_request(&self, request: &http::Request, route: Option<Route<'_>>, context: &C) {
        let _ = context;

        emit!(
            self.level,
            method = %request.method(),
            matched_path = route.map_or(UNMATCHED, |route| route.path()),
            operation_id = route.map_or(UNMATCHED, |route| route.operation_id()),
            request_id = request_id(request.headers()),
            headers = self.recorded(request.headers()),
            "request received",
        );
    }

    fn on_response(
        &self,
        response: &http::Response,
        route: Option<Route<'_>>,
        elapsed: std::time::Duration,
    ) {
        emit!(
            self.level,
            matched_path = route.map_or(UNMATCHED, |route| route.path()),
            operation_id = route.map_or(UNMATCHED, |route| route.operation_id()),
            status = response.status().as_u16(),
            latency = ?elapsed,
            request_id = request_id(response.headers()),
            "response sent",
        );
    }

    fn on_panic(&self, payload: &(dyn std::any::Any + Send), route: Option<Route<'_>>) {
        // Always at `ERROR`, whatever the configured level: a panic is not
        // routine traffic, and a service that hid one behind a filter would be
        // hiding the one line worth keeping.
        tracing::error!(
            matched_path = route.map_or(UNMATCHED, |route| route.path()),
            operation_id = route.map_or(UNMATCHED, |route| route.operation_id()),
            panic = panic_message(payload),
            "handler panicked",
        );
    }
}

/// What a panic payload says, when it says anything.
///
/// A payload is `Any`, and only the two shapes `panic!` produces carry a
/// message; anything else is reported as having none rather than as nothing
/// having happened.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message
    } else {
        "<non-string panic payload>"
    }
}
