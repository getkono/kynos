//! Request tracing: the one standard way to log at the operation level.

use crate::{http, middleware::Observer, router::operation::Route};

/// Emits a `tracing` event for each end of an operation.
///
/// Both carry `method`, `matched_path`, `operation_id` and `request_id`; the
/// closing one adds `status` and `latency`. Handler bodies use plain
/// `tracing::info!` and inherit whatever span the application established.
///
/// Observers run before any interceptor, so the opening event precedes
/// [`RequestId`](super::request_id::RequestId) assigning an identifier. Its
/// `request_id` is the one the client sent where
/// [`correlating`](Trace::correlating) names a `RequestId` that echoes it, and
/// empty otherwise, since an identifier `RequestId` replaces is not the
/// request's. The closing event carries the identifier the response does.
///
/// `matched_path` is exactly the `paths` key from the description, so it is a
/// bounded-cardinality metric label.
///
/// Two events rather than one span, because an [`Observer`] holds nothing
/// between the request arriving and the response leaving.
///
/// Choosing a subscriber remains the application's decision.
#[derive(Clone, Debug)]
pub struct Trace {
    level: tracing::Level,
    recorded: &'static [&'static str],
    correlation: &'static [&'static str],
    trust_client: bool,
}

/// Emits an event at a level chosen at run time.
///
/// `tracing`'s macros bake the level into a `static` callsite.
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

/// What an unmatched request has instead of a `paths` key, so every log line
/// has the same shape.
const UNMATCHED: &str = "<unmatched>";

/// The correlation field names [`Trace`] reads unless told others; the ones
/// [`XRequestId`](super::request_id::XRequestId) declares.
const DEFAULT_CORRELATION: &[&str] = &["x-request-id"];

/// Header names recorded as present and never by value.
///
/// Fixed: an application cannot widen or narrow it. Compared
/// case-insensitively, per RFC 9110 section 5.1.
pub const REDACTED: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
];

impl Trace {
    /// Traces every operation at `INFO`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            level: tracing::Level::INFO,
            recorded: &[],
            correlation: DEFAULT_CORRELATION,
            trust_client: false,
        }
    }

    /// Correlates by the identifier `request_id` assigns.
    ///
    /// Pass the [`RequestId`](super::request_id::RequestId) the router mounts:
    /// the header names and whether an inbound identifier is echoed are read
    /// from it, so the two cannot disagree. Without this, `Trace` assumes
    /// [`RequestId::new`](super::request_id::RequestId::new).
    ///
    /// ```
    /// use kynos::middleware::{request_id::RequestId, trace::Trace};
    ///
    /// let request_id = RequestId::new().trust_client(true);
    /// let trace = Trace::new()
    ///     .level(tracing::Level::DEBUG)
    ///     .correlating(&request_id);
    /// # let _ = trace;
    /// ```
    #[must_use]
    pub fn correlating<S, G: super::request_id::CorrelationHeaders>(
        mut self,
        request_id: &super::request_id::RequestId<S, G>,
    ) -> Self {
        self.correlation = G::NAMES;
        self.trust_client = request_id.trust_client;
        self
    }

    /// The identifier the opening event carries: the inbound one `RequestId`
    /// echoes, under the first declared name present as it reads it, or none.
    fn inbound<'a>(&self, headers: &'a http::HeaderMap) -> &'a str {
        if !self.trust_client {
            return "";
        }

        self.correlation
            .iter()
            .find_map(|name| headers.get(*name))
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
    }

    /// The identifier the closing event carries: the one the response does,
    /// which `RequestId` writes under every declared name.
    fn assigned<'a>(&self, headers: &'a http::HeaderMap) -> &'a str {
        self.correlation
            .first()
            .and_then(|name| headers.get(*name))
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
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
    /// Anything not listed is omitted. A header on [`REDACTED`] is recorded as
    /// present and never by value.
    #[must_use]
    pub fn record_headers(mut self, names: &'static [&'static str]) -> Self {
        self.recorded = names;
        self
    }

    /// The listed headers this request carries, as one field value.
    ///
    /// One field, since a `tracing` field name is fixed at its callsite.
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
            if REDACTED
                .iter()
                .any(|secret| secret.eq_ignore_ascii_case(name))
            {
                recorded.push_str("<redacted>");
            } else {
                recorded.push_str(value);
            }
        }

        recorded
    }
}

impl Default for Trace {
    fn default() -> Self {
        Self::new()
    }
}

impl<C> Observer<C> for Trace {
    fn on_request(&self, request: &http::Request, route: Option<Route<'_>>, context: &C) {
        let _ = context;

        emit!(
            self.level,
            method = %request.method(),
            matched_path = route.map_or(UNMATCHED, |route| route.path()),
            operation_id = route.map_or(UNMATCHED, |route| route.operation_id()),
            request_id = self.inbound(request.headers()),
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
            request_id = self.assigned(response.headers()),
            "response sent",
        );
    }

    fn on_panic(&self, payload: &(dyn std::any::Any + Send), route: Option<Route<'_>>) {
        // Always `ERROR`, so no level filter hides a panic.
        tracing::error!(
            matched_path = route.map_or(UNMATCHED, |route| route.path()),
            operation_id = route.map_or(UNMATCHED, |route| route.operation_id()),
            panic = panic_message(payload),
            "handler panicked",
        );
    }
}

/// What a panic payload says, when it is one of the two shapes `panic!` makes.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message
    } else {
        "<non-string panic payload>"
    }
}

#[cfg(test)]
mod tests;
