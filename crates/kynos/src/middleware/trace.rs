//! Request tracing: the one standard way to log at the operation level.

use crate::{http, middleware::Observer, router::operation::Route};

/// Emits a `tracing` event for each end of an operation.
///
/// Both carry `method`, `matched_path`, `operation_id` and `request_id`; the
/// closing one adds `status` and `latency`. The opening event's `method` is the
/// request's; the closing one's is the matched operation's, so it joins with
/// `operation_id` on its own line: `GET` for a `HEAD` that a `GET` answers, and
/// the same placeholder as `matched_path` where nothing matched.
///
/// Handler bodies use plain `tracing::info!` and inherit whatever span the
/// application established. A 5xx a derived `ApiError` answers with logs its
/// cause there too, on the `kynos::error` target and always at `ERROR`; the
/// closing event here carries only its status.
///
/// Observers run before any interceptor, so the opening event precedes
/// [`RequestId`](super::request_id::RequestId) assigning an identifier. Its
/// `request_id` is the one the client sent where
/// [`correlating`](Trace::correlating) names a `RequestId` that echoes it, and
/// empty otherwise, since an identifier `RequestId` replaces is not the
/// request's. The closing event carries the identifier the response does.
///
/// The two agree only on operations that `RequestId` covers and reaches. The
/// opening event cannot know whether it will: where a trusting `RequestId` is
/// mounted on a group the operation is outside of, or an earlier interceptor
/// answers first, the opening event names the client's identifier and the
/// closing one is empty.
///
/// `matched_path` is exactly the `paths` key from the description, so it is a
/// bounded-cardinality metric label.
///
/// Two events rather than one span, because an [`Observer`] holds nothing
/// between the request arriving and the response leaving. [`span`](Trace::span)
/// turns the same configuration into an interceptor that opens one span the
/// chain's own events inherit.
///
/// Choosing a subscriber remains the application's decision.
#[derive(Clone, Debug)]
pub struct Trace {
    level: Level,
    recorded: &'static [&'static str],
    correlation: &'static [&'static str],
    trust_client: bool,
}

/// The level [`Trace`] emits its events at.
///
/// Kynos's own type rather than `tracing`'s, so that no `tracing` item appears
/// in a public signature. Each variant is emitted as the `tracing` level of
/// the same name, which is what a subscriber filters on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Level {
    /// Emitted as `tracing`'s `TRACE`.
    Trace,
    /// Emitted as `tracing`'s `DEBUG`.
    Debug,
    /// Emitted as `tracing`'s `INFO`.
    Info,
    /// Emitted as `tracing`'s `WARN`.
    Warn,
    /// Emitted as `tracing`'s `ERROR`.
    Error,
}

/// Emits an event at a level chosen at run time.
///
/// `tracing`'s macros bake the level into a `static` callsite, so each
/// [`Level`] gets its own.
macro_rules! emit {
    ($level:expr, $($event:tt)*) => {
        match $level {
            Level::Error => tracing::event!(tracing::Level::ERROR, $($event)*),
            Level::Warn => tracing::event!(tracing::Level::WARN, $($event)*),
            Level::Info => tracing::event!(tracing::Level::INFO, $($event)*),
            Level::Debug => tracing::event!(tracing::Level::DEBUG, $($event)*),
            Level::Trace => tracing::event!(tracing::Level::TRACE, $($event)*),
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
    /// Traces every operation at [`Level::Info`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            level: Level::Info,
            recorded: &[],
            correlation: DEFAULT_CORRELATION,
            trust_client: false,
        }
    }

    /// Correlates by the identifier `request_id` assigns.
    ///
    /// Pass the [`RequestId`](super::request_id::RequestId) the router mounts:
    /// the header names and whether an inbound identifier is echoed are read
    /// from it, so the two cannot disagree on either setting. Which operations
    /// it covers is not read: `Trace` observes every one, so see [`Trace`] for
    /// where the two events can still differ. Without this, `Trace` assumes
    /// [`RequestId::new`](super::request_id::RequestId::new).
    ///
    /// ```
    /// use kynos::middleware::{
    ///     request_id::RequestId,
    ///     trace::{Level, Trace},
    /// };
    ///
    /// let request_id = RequestId::new().trust_client(true);
    /// let trace = Trace::new()
    ///     .level(Level::Debug)
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

        self.present(headers)
    }

    /// The identifier `headers` carry under the first correlation name
    /// present, or none.
    fn present<'a>(&self, headers: &'a http::HeaderMap) -> &'a str {
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
    pub fn level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    /// Records request headers matching these names on the `request received`
    /// event; the other events carry none.
    ///
    /// Anything not listed is omitted. Of a field the request repeats, only
    /// the first line is recorded, and a line that is not visible ASCII is
    /// skipped, as if absent. A header on [`REDACTED`] is recorded as present
    /// and never by value.
    #[must_use]
    pub fn record_headers(mut self, names: &'static [&'static str]) -> Self {
        self.recorded = names;
        self
    }

    /// Wraps everything mounted inside it in one span named `request`, which
    /// the events beneath it inherit: an inner interceptor's, a security
    /// scheme's and the handler's.
    ///
    /// The span opens with `method`, `matched_path`, `operation_id` and
    /// `request_id` as the events here carry them, and `headers` as
    /// [`record_headers`](Trace::record_headers) selects them, and closes with
    /// `status` and `latency_ms`, the milliseconds the rest of the chain took,
    /// fractional. Each of `fields` is declared on it empty, so the application
    /// can record into it later, as an authenticator recording the user it
    /// admitted with `tracing::Span::current().record("user_id", id)`. A name
    /// listed twice, or one of the span's own, is declared once.
    ///
    /// It is an [`Interceptor`](super::Interceptor), so it covers what is
    /// mounted inside it and nothing outside; mount it first to cover the
    /// whole chain. `request_id` is the identifier the request carries when
    /// the span opens, so mount it after the [`RequestId`] that
    /// [`correlating`](Trace::correlating) names: the identifier is then the
    /// one `RequestId` assigned, and before it, whatever the client sent.
    /// A request the client abandons before the chain answers closes the span
    /// without `status` or `latency_ms`.
    ///
    /// The level set by [`level`](Trace::level) is the span's; a subscriber
    /// that filters the span out leaves the events inside under whatever span
    /// was current.
    ///
    /// ```
    /// use kynos::{
    ///     middleware::{request_id::RequestId, trace::Trace},
    ///     prelude::*,
    /// };
    ///
    /// let request_id = RequestId::new();
    /// let span = Trace::new()
    ///     .correlating(&request_id)
    ///     .span(&["user_id", "session_id"]);
    ///
    /// // The first `intercept` is the outermost, so the span sees the
    /// // identifier `RequestId` assigns.
    /// let router = Router::<()>::new().intercept(request_id).intercept(span);
    /// # let _ = router;
    /// ```
    ///
    /// [`RequestId`]: super::request_id::RequestId
    #[must_use]
    pub fn span(self, fields: &'static [&'static str]) -> TraceSpan {
        TraceSpan {
            callsite: SpanCallsite::interned(self.level, fields),
            trace: self,
        }
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
            method = %route.map_or(UNMATCHED, |route| route.method().as_wire_str()),
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

/// One `request` span around the rest of the chain; see [`Trace::span`].
///
/// Reads, adds and answers nothing, so mounting it changes no operation's
/// description.
#[derive(Clone)]
pub struct TraceSpan {
    trace: Trace,
    callsite: &'static SpanCallsite,
}

impl std::fmt::Debug for TraceSpan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TraceSpan")
            .field("trace", &self.trace)
            .field("fields", &self.callsite.declared)
            .finish()
    }
}

/// The fields every `request` span declares, in the order its callsite does.
///
/// [`TraceSpan::open`] reads the first five by position.
const SPAN_FIELDS: &[&str] = &[
    "method",
    "matched_path",
    "operation_id",
    "request_id",
    "headers",
    "status",
    "latency_ms",
];

/// A `request` span's callsite, built at run time since its fields are the
/// application's.
///
/// `tracing`'s macros declare a callsite as a `static` whose field names are
/// fixed where it is written, so one naming caller-declared fields has to be
/// built and leaked instead. [`interned`](SpanCallsite::interned) builds one
/// per distinct level and field list, so assembling a router again leaks
/// nothing more.
struct SpanCallsite {
    /// Set once, right after the callsite has an address to identify it by.
    metadata: std::sync::OnceLock<tracing::Metadata<'static>>,
    /// What the dispatchers last said of it, as [`Interest`](tracing::subscriber::Interest)
    /// cannot be stored atomically.
    interest: std::sync::atomic::AtomicU8,
    level: Level,
    /// The caller's list as given, which `interned` compares by content.
    declared: &'static [&'static str],
}

/// [`SpanCallsite::interest`]'s three values.
const NEVER: u8 = 0;
const SOMETIMES: u8 = 1;
const ALWAYS: u8 = 2;

impl SpanCallsite {
    /// The callsite for `level` and `declared`, built and registered the first
    /// time the pair is asked for.
    fn interned(level: Level, declared: &'static [&'static str]) -> &'static Self {
        use std::sync::{Mutex, PoisonError};

        static INTERNED: Mutex<Vec<&'static SpanCallsite>> = Mutex::new(Vec::new());

        // A panic while holding the list leaves it whole: each entry is pushed
        // only once it is registered.
        let mut interned = INTERNED.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(callsite) = interned
            .iter()
            .find(|callsite| callsite.level == level && callsite.declared == declared)
        {
            return callsite;
        }

        let mut names = SPAN_FIELDS.to_vec();
        for name in declared {
            if !names.contains(name) {
                names.push(name);
            }
        }
        let names: &'static [&'static str] = Box::leak(names.into_boxed_slice());

        let callsite: &'static Self = Box::leak(Box::new(Self {
            metadata: std::sync::OnceLock::new(),
            interest: std::sync::atomic::AtomicU8::new(SOMETIMES),
            level,
            declared,
        }));
        let metadata = tracing::Metadata::new(
            "request",
            module_path!(),
            level.as_tracing(),
            Some(file!()),
            Some(line!()),
            Some(module_path!()),
            tracing::field::FieldSet::new(names, tracing::callsite::Identifier(callsite)),
            tracing::metadata::Kind::SPAN,
        );
        if callsite.metadata.set(metadata).is_err() {
            unreachable!("the callsite was built just now");
        }
        tracing::callsite::register(callsite);

        interned.push(callsite);
        callsite
    }

    fn static_metadata(&'static self) -> &'static tracing::Metadata<'static> {
        self.metadata
            .get()
            .expect("set before the callsite is handed out")
    }

    /// Whether a span opened here now would be recorded, decided as
    /// `tracing`'s own `span!` decides it.
    fn enabled(&'static self) -> bool {
        use tracing::level_filters::{LevelFilter, STATIC_MAX_LEVEL};

        let metadata = self.static_metadata();
        let level = *metadata.level();
        if level > STATIC_MAX_LEVEL || level > LevelFilter::current() {
            return false;
        }

        match self.interest.load(std::sync::atomic::Ordering::Relaxed) {
            NEVER => false,
            ALWAYS => true,
            _ => tracing::dispatcher::get_default(|dispatch| dispatch.enabled(metadata)),
        }
    }
}

impl tracing::callsite::Callsite for SpanCallsite {
    fn set_interest(&self, interest: tracing::subscriber::Interest) {
        let interest = if interest.is_never() {
            NEVER
        } else if interest.is_always() {
            ALWAYS
        } else {
            SOMETIMES
        };
        self.interest
            .store(interest, std::sync::atomic::Ordering::Relaxed);
    }

    fn metadata(&self) -> &tracing::Metadata<'_> {
        self.metadata
            .get()
            .expect("set before the callsite is registered")
    }
}

impl Level {
    /// The `tracing` level of the same name.
    fn as_tracing(self) -> tracing::Level {
        match self {
            Self::Error => tracing::Level::ERROR,
            Self::Warn => tracing::Level::WARN,
            Self::Info => tracing::Level::INFO,
            Self::Debug => tracing::Level::DEBUG,
            Self::Trace => tracing::Level::TRACE,
        }
    }
}

impl TraceSpan {
    /// The span for `request`, or a disabled one where nothing listens.
    fn open(&self, request: &http::Request, route: Option<Route<'_>>) -> tracing::Span {
        use tracing::field::Value;

        if !self.callsite.enabled() {
            return tracing::Span::none();
        }

        let metadata = self.callsite.static_metadata();
        let fields = metadata.fields();
        let mut declared = fields.iter();
        let mut next = || {
            declared
                .next()
                .expect("every `SPAN_FIELDS` name is declared")
        };
        let (method, matched_path, operation_id, request_id, headers) =
            (next(), next(), next(), next(), next());

        let recorded =
            (!self.trace.recorded.is_empty()).then(|| self.trace.recorded(request.headers()));

        tracing::Span::new(
            metadata,
            &fields.value_set(&[
                (&method, Some(&request.method().as_str() as &dyn Value)),
                (
                    &matched_path,
                    Some(&route.map_or(UNMATCHED, |route| route.path()) as &dyn Value),
                ),
                (
                    &operation_id,
                    Some(&route.map_or(UNMATCHED, |route| route.operation_id()) as &dyn Value),
                ),
                (
                    &request_id,
                    Some(&self.trace.present(request.headers()) as &dyn Value),
                ),
                (
                    &headers,
                    recorded.as_ref().map(|recorded| recorded as &dyn Value),
                ),
            ]),
        )
    }
}

impl<C: Sync + 'static> super::Interceptor<C> for TraceSpan {
    type Reads = ();
    type Adds = ();
    type Short = std::convert::Infallible;

    // The span is entered around `next.run` as well as around its future, so
    // the inner interceptors' work before their futures is inside it too.
    fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: super::Next<'_, C>,
    ) -> impl Future<Output = Result<super::Continued, std::convert::Infallible>> + Send {
        use tracing::Instrument;

        let _ = (reads, context);
        let started = std::time::Instant::now();
        let span = self.open(&request, next.route());
        let run = span.in_scope(|| next.run(request)).instrument(span);

        async move {
            let mut run = std::pin::pin!(run);
            let continued = run.as_mut().await;
            let span = run.span();
            span.record("status", continued.status().as_u16());
            span.record("latency_ms", started.elapsed().as_secs_f64() * 1000.0);

            Ok(continued)
        }
    }
}

/// The event a derived `ApiError`'s 5xx emits through
/// [`Failed`](crate::__private::problem::Failed), and the owned chain it
/// records.
pub(crate) mod reported {
    use std::{error::Error, fmt};

    /// The target the event is filtered by: the module the policy is
    /// documented in, rather than this one, which is not where it is declared.
    pub(crate) const TARGET: &str = "kynos::error";

    /// A failure's message and its sources', owned, so a chain borrowed from
    /// an error with a lifetime is recorded as the `'static` error `tracing`
    /// asks for.
    #[derive(Debug)]
    pub(crate) struct Chain {
        message: String,
        source: Option<Box<Chain>>,
    }

    impl Chain {
        /// `error`'s message, then each `source()` in turn.
        pub(crate) fn of<E: Error + ?Sized>(error: &E) -> Self {
            let mut messages = vec![error.to_string()];
            let mut next = error.source();
            while let Some(source) = next {
                messages.push(source.to_string());
                next = source.source();
            }
            let mut chain = None;
            while let Some(message) = messages.pop() {
                chain = Some(Box::new(Self {
                    message,
                    source: chain,
                }));
            }
            *chain.expect("the failure's own message is the first")
        }

        /// A failure with no `source()` to walk: its message alone.
        pub(crate) fn alone<E: fmt::Display + ?Sized>(failure: &E) -> Self {
            Self {
                message: failure.to_string(),
                source: None,
            }
        }
    }

    impl fmt::Display for Chain {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(&self.message)
        }
    }

    impl Error for Chain {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            self.source.as_deref().map(|source| source as _)
        }
    }

    /// One `ERROR` event, whatever level `Trace` is set to, as a panic's is:
    /// a server failure is never filtered out by the request log's verbosity.
    /// The chain is built only where a subscriber listens: `tracing` evaluates
    /// a field only once the event is enabled.
    pub(crate) fn emit(status: u16, type_uri: &'static str, chain: impl FnOnce() -> Chain) {
        tracing::error!(
            target: TARGET,
            status,
            problem_type = type_uri,
            error = &chain() as &(dyn Error + 'static),
            "operation failed",
        );
    }
}

#[cfg(test)]
mod tests;
