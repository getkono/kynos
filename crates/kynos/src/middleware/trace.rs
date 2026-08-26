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
    correlation: &'static str,
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

/// The correlation field name [`Trace`] reads unless told another.
///
/// The same name [`XRequestId`](super::request_id::XRequestId) declares, and
/// the reason [`Trace::correlating`] exists is so that agreement is checked
/// rather than assumed.
const DEFAULT_CORRELATION: &str = "x-request-id";

/// Header names recorded as present and never by value.
///
/// Not a policy an application can widen or narrow. A denylist that can be
/// switched off is one that will be, and the cost of being wrong here is a
/// credential in a log file that outlives the request by months.
///
/// Compared case-insensitively, per RFC 9110 section 5.1.
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
        }
    }

    /// Reads the correlation identifier from the group `G` declares.
    ///
    /// [`RequestId`](super::request_id::RequestId) is generic over its
    /// correlation group precisely so the field name can be something other
    /// than `x-request-id`, and this observer had that name written into it a
    /// second time. Swapping the group then left every event logging an empty
    /// `request_id`, which is the one field the observer exists to correlate
    /// on.
    ///
    /// The name is read from `G::NAMES` rather than passed as a string, so the
    /// two sides cannot disagree: it is the same `const` the interceptor
    /// declares and the conflict check compares.
    ///
    /// ```
    /// use kynos::middleware::{request_id::XRequestId, trace::Trace};
    ///
    /// let trace = Trace::new()
    ///     .level(tracing::Level::DEBUG)
    ///     .correlating::<XRequestId>();
    /// # let _ = trace;
    /// ```
    #[must_use]
    pub fn correlating<G: super::request_id::CorrelationHeaders>(mut self) -> Self {
        self.correlation = G::NAMES.first().copied().unwrap_or(DEFAULT_CORRELATION);
        self
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
    /// Anything not listed is omitted, so a header carrying a credential cannot
    /// end up in a log by accident. A header on [`REDACTED`] is recorded as
    /// present and never by value, so listing one on purpose does not put a
    /// secret in a log either.
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
            if REDACTED
                .iter()
                .any(|secret| secret.eq_ignore_ascii_case(name))
            {
                // Present, and never by value. Whether the field arrived is
                // usually why it was listed; what it carried is never worth a
                // log line, and `security.md` already reaches for a
                // constant-time compare one module away rather than let a
                // secret leak through a comparison.
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

/// The correlation identifier a request or response carries, if any.
fn request_id<'a>(headers: &'a http::HeaderMap, name: &str) -> &'a str {
    headers
        .get(name)
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
            request_id = request_id(request.headers(), self.correlation),
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
            request_id = request_id(response.headers(), self.correlation),
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

#[cfg(test)]
mod tests {
    use super::{DEFAULT_CORRELATION, REDACTED, Trace};
    use crate::{
        extract::params::header::HeaderParams,
        http::{HeaderMap, HeaderValue},
        middleware::request_id::XRequestId,
    };

    /// A header map from pairs.
    fn map(fields: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in fields {
            headers.append(
                crate::http::HeaderName::from_bytes(name.as_bytes()).expect("a legal field name"),
                HeaderValue::from_str(value).expect("a printable field"),
            );
        }
        headers
    }

    /// Every name on the denylist is recorded as present and never by value.
    ///
    /// A sweep of the whole list rather than a case for `authorization`: the
    /// list is the guarantee, and a name added to it without being covered here
    /// would be a name nothing checks.
    #[test]
    fn every_redacted_header_is_recorded_without_its_value() {
        for name in REDACTED {
            let trace = Trace::new().record_headers(std::slice::from_ref(name));
            let recorded = trace.recorded(&map(&[(name, "s3cret-value")]));

            assert!(
                recorded.contains("<redacted>"),
                "`{name}` was recorded verbatim: {recorded}"
            );
            assert!(
                !recorded.contains("s3cret-value"),
                "`{name}` leaked its value: {recorded}"
            );
        }
    }

    /// The case a denylist must not break: an ordinary header still records.
    ///
    /// Without this the test above passes for a `recorded` that redacts
    /// everything, which would make the feature useless rather than safe.
    #[test]
    fn an_ordinary_header_is_recorded_with_its_value() {
        let trace = Trace::new().record_headers(&["x-tenant"]);
        let recorded = trace.recorded(&map(&[("x-tenant", "acme")]));

        assert_eq!(recorded, "x-tenant=acme");
    }

    /// The denylist is matched case-insensitively, per RFC 9110 section 5.1.
    #[test]
    fn a_redacted_name_in_another_case_is_still_redacted() {
        let trace = Trace::new().record_headers(&["Authorization"]);
        let recorded = trace.recorded(&map(&[("authorization", "Bearer eyJ")]));

        assert!(recorded.contains("<redacted>"), "{recorded}");
        assert!(!recorded.contains("eyJ"), "{recorded}");
    }

    /// The correlation name comes from the group, not from a second copy of it.
    #[test]
    fn the_correlation_name_is_read_from_the_group() {
        assert_eq!(XRequestId::NAMES, [DEFAULT_CORRELATION]);
        assert_eq!(
            Trace::new().correlating::<XRequestId>().correlation,
            DEFAULT_CORRELATION
        );
    }
}
