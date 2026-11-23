use std::{
    future::Future,
    pin::{Pin, pin},
    sync::Arc,
    task::{Context, Poll, Waker},
};

use super::{DEFAULT_CORRELATION, Level, REDACTED, SPAN_FIELDS, SpanCallsite, Trace};
use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams, UnencodableHeader},
    http::{HeaderMap, HeaderValue, Request, Response, StatusCode, body::Body},
    middleware::{
        Next, Observer,
        erased::{ErasedInterceptor, ErasedTerminal},
        request_id::{CorrelationHeaders, RequestId, XRequestId},
    },
    router::operation::Route,
};
use kynos_openapi::Method;

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

/// The default correlation names are the default group's, not a second copy.
#[test]
fn the_default_correlation_names_are_the_default_groups() {
    assert_eq!(XRequestId::NAMES, DEFAULT_CORRELATION);
}

/// The names and the trust both come from the `RequestId` correlated by.
#[test]
fn correlating_reads_the_group_and_the_trust_from_the_request_id() {
    let trace = Trace::new().correlating(&RequestId::new().header::<Pair>().trust_client(true));

    assert_eq!(trace.correlation, Pair::NAMES);
    assert!(trace.trust_client);
    assert!(!Trace::new().correlating(&RequestId::new()).trust_client);
}

/// A `RequestId` that replaces an inbound identifier leaves the opening event
/// without one, since what the client sent is not the request's identifier.
#[test]
fn an_identifier_request_id_replaces_is_not_logged_on_arrival() {
    let inbound = map(&[("x-request-id", "forged")]);

    assert_eq!(Trace::new().inbound(&inbound), None);
    assert_eq!(
        Trace::new()
            .correlating(&RequestId::new().trust_client(false))
            .inbound(&inbound),
        None
    );
}

/// A `RequestId` that echoes an inbound identifier makes it the request's, so
/// the opening event carries it, read as `RequestId` reads it: the first
/// declared name present.
#[test]
fn an_identifier_request_id_echoes_is_logged_on_arrival() {
    let trusting = Trace::new().correlating(&RequestId::new().header::<Pair>().trust_client(true));

    assert_eq!(
        trusting.inbound(&map(&[("x-trace-id", "second")])),
        Some("second")
    );
    assert_eq!(
        trusting.inbound(&map(&[
            ("x-trace-id", "second"),
            ("x-correlation-id", "first")
        ])),
        Some("first")
    );
    assert_eq!(trusting.inbound(&map(&[])), None);
    assert_eq!(trusting.inbound(&map(&[("x-correlation-id", "")])), None);
}

/// The lines a `fmt` subscriber writes while `emitting` runs on this thread,
/// each span's closing included.
fn captured(emitting: impl FnOnce()) -> String {
    captured_at(tracing::Level::TRACE, emitting)
}

/// The lines a `fmt` subscriber admitting `max` and below writes while
/// `emitting` runs on this thread, each span's closing included.
fn captured_at(max: tracing::Level, emitting: impl FnOnce()) -> String {
    #[derive(Clone, Default)]
    struct Sink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("an unpoisoned sink")
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let sink = Sink::default();
    let writer = sink.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(max)
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
        .with_ansi(false)
        .without_time()
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, emitting);

    let bytes = sink.0.lock().expect("an unpoisoned sink").clone();
    String::from_utf8(bytes).expect("UTF-8 log lines")
}

/// The opening event carries the identifier `inbound` reads, at the level set.
#[test]
fn the_opening_event_carries_the_echoed_identifier() {
    let trace = Trace::new()
        .level(Level::Debug)
        .correlating(&RequestId::new().trust_client(true));
    let mut request = crate::http::Request::new(crate::http::body::Body::empty());
    *request.headers_mut() = map(&[("x-request-id", "echoed")]);

    let lines = captured(|| Observer::<()>::on_request(&trace, &request, None, &()));

    assert!(lines.contains("DEBUG"), "{lines}");
    assert!(lines.contains("request received"), "{lines}");
    assert!(lines.contains("request_id=\"echoed\""), "{lines}");
}

/// The opening event leaves `request_id` out when it has no identifier,
/// rather than logging an empty one: neither for a `RequestId` that replaces
/// what the client sent, nor for a trusting one the client sent nothing to.
#[test]
fn the_opening_event_omits_an_identifier_it_has_none_of() {
    let mut forged = crate::http::Request::new(crate::http::body::Body::empty());
    *forged.headers_mut() = map(&[("x-request-id", "forged")]);
    let bare = crate::http::Request::new(crate::http::body::Body::empty());
    let trusting = Trace::new().correlating(&RequestId::new().trust_client(true));

    for (trace, request) in [(Trace::new(), &forged), (trusting, &bare)] {
        let lines = captured(|| Observer::<()>::on_request(&trace, request, None, &()));

        assert!(lines.contains("request received"), "{lines}");
        assert!(!lines.contains("request_id"), "{lines}");
    }
}

/// Each `Level` is emitted as the `tracing` level of the same name, on both
/// events.
#[test]
fn each_level_is_emitted_as_the_tracing_level_of_its_name() {
    let levels = [
        (Level::Trace, "TRACE"),
        (Level::Debug, "DEBUG"),
        (Level::Info, "INFO"),
        (Level::Warn, "WARN"),
        (Level::Error, "ERROR"),
    ];
    let request = crate::http::Request::new(crate::http::body::Body::empty());
    let response = crate::http::Response::new(crate::http::body::Body::empty());

    for (level, name) in levels {
        let trace = Trace::new().level(level);
        let lines = captured(|| {
            Observer::<()>::on_request(&trace, &request, None, &());
            Observer::<()>::on_response(&trace, &response, None, std::time::Duration::ZERO);
        });

        let prefixes: Vec<&str> = lines
            .lines()
            .map(|line| line.split_whitespace().next().unwrap_or_default())
            .collect();
        assert_eq!(prefixes, [name, name], "{lines}");
    }
}

/// A rule that raises 429 and a 5xx apart from the rest.
fn by_status(status: StatusCode) -> Level {
    match status.as_u16() {
        500.. => Level::Error,
        429 => Level::Warn,
        _ => Level::Debug,
    }
}

/// The level each event of one exchange answered with `status` is emitted at.
fn levels_of(trace: &Trace, status: StatusCode) -> Vec<String> {
    let request = Request::new(Body::empty());
    let mut response = Response::new(Body::empty());
    *response.status_mut() = status;

    let lines = captured(|| {
        Observer::<()>::on_request(trace, &request, None, &());
        Observer::<()>::on_response(trace, &response, None, std::time::Duration::ZERO);
    });

    lines
        .lines()
        .map(|line| {
            line.split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

/// `level_for` picks the closing event's level from the status sent, and
/// leaves the opening event, and a span, at `level`.
#[test]
fn the_closing_level_is_chosen_by_status_and_the_opening_one_is_not() {
    let trace = Trace::new().level(Level::Trace).level_for(by_status);

    assert_eq!(levels_of(&trace, StatusCode::OK), ["TRACE", "DEBUG"]);
    assert_eq!(levels_of(&trace, StatusCode::NOT_FOUND), ["TRACE", "DEBUG"]);
    assert_eq!(
        levels_of(&trace, StatusCode::TOO_MANY_REQUESTS),
        ["TRACE", "WARN"]
    );
    assert_eq!(
        levels_of(&trace, StatusCode::INTERNAL_SERVER_ERROR),
        ["TRACE", "ERROR"]
    );
    assert_eq!(trace.span(&[]).callsite.level, Level::Trace);
}

/// `level` set after `level_for` moves the opening event and leaves the
/// rule in charge of the closing one, so the two read the same in either
/// order.
#[test]
fn level_after_level_for_does_not_replace_the_rule() {
    let trace = Trace::new().level_for(by_status).level(Level::Info);

    assert_eq!(
        levels_of(&trace, StatusCode::SERVICE_UNAVAILABLE),
        ["INFO", "ERROR"]
    );
    assert_eq!(levels_of(&trace, StatusCode::OK), ["INFO", "DEBUG"]);
}

/// The closing event carries the identifier `RequestId` assigned, and its
/// status, under whichever group it writes: this `Trace` names no group, and
/// assumes the default's, which the response does not carry.
#[test]
fn the_closing_event_carries_the_assigned_identifier() {
    let response = served(
        &[Arc::new(RequestId::new().header::<Pair>())],
        None,
        request(&[]),
    );
    assert!(response.headers().get("x-request-id").is_none());

    let lines = captured(|| {
        Observer::<()>::on_response(&Trace::new(), &response, None, std::time::Duration::ZERO);
    });

    assert!(lines.contains("INFO"), "{lines}");
    assert!(lines.contains("response sent"), "{lines}");
    assert!(lines.contains("status=201"), "{lines}");
    assert!(lines.contains("request_id=\"0\""), "{lines}");
}

/// A field named as the identifier's, which no `RequestId` wrote, is not
/// logged as one: only an assigned identifier is the request's.
#[test]
fn the_closing_event_ignores_an_identifier_no_request_id_assigned() {
    let mut response = Response::new(Body::empty());
    *response.headers_mut() = map(&[("x-request-id", "forged")]);

    let lines = captured(|| {
        Observer::<()>::on_response(&Trace::new(), &response, None, std::time::Duration::ZERO);
    });

    assert!(lines.contains("request_id=\"\""), "{lines}");
}

/// The closing event names the matched operation's method, so it joins on its
/// own, and the placeholder where nothing matched, so its shape never varies.
#[test]
fn the_closing_event_carries_the_method() {
    let response = crate::http::Response::new(crate::http::body::Body::empty());
    let route = Route::new("/users/{id}", "deleteUser", Method::Delete);

    let matched = captured(|| {
        Observer::<()>::on_response(
            &Trace::new(),
            &response,
            Some(route),
            std::time::Duration::ZERO,
        );
    });
    let unmatched = captured(|| {
        Observer::<()>::on_response(&Trace::new(), &response, None, std::time::Duration::ZERO);
    });

    assert!(matched.contains("method=DELETE "), "{matched}");
    assert!(unmatched.contains("method=<unmatched> "), "{unmatched}");
}

/// Several recorded headers go out as one field, joined in the order they
/// were listed rather than the order the request sent them, with a listed
/// header the request lacks left out and a redacted one marked.
#[test]
fn recorded_headers_are_joined_in_the_order_listed() {
    let trace = Trace::new().record_headers(&["x-tenant", "x-absent", "authorization", "x-region"]);
    let recorded = trace.recorded(&map(&[
        ("x-region", "eu"),
        ("authorization", "Bearer eyJ"),
        ("x-tenant", "acme"),
    ]));

    assert_eq!(
        recorded,
        "x-tenant=acme, authorization=<redacted>, x-region=eu"
    );
}

/// A recovered panic is one `ERROR` event naming the matched operation and
/// what the payload says — whatever level the rest are emitted at, and for
/// each payload shape `panic!` makes as well as one it does not.
#[test]
fn a_panic_is_reported_at_error_with_its_operation_and_message() {
    let trace = Trace::new().level(Level::Debug);
    let route = Route::new("/users/{id}", "deleteUser", Method::Delete);
    let payloads: [(Box<dyn std::any::Any + Send>, &str); 3] = [
        (Box::new("a static message"), "a static message"),
        (
            Box::new(String::from("a formatted message")),
            "a formatted message",
        ),
        (Box::new(42_u32), "<non-string panic payload>"),
    ];

    for (payload, message) in payloads {
        let lines = captured(|| Observer::<()>::on_panic(&trace, payload.as_ref(), Some(route)));

        assert_eq!(
            lines,
            format!(
                "ERROR kynos::middleware::trace: handler panicked \
                 matched_path=\"/users/{{id}}\" operation_id=\"deleteUser\" panic=\"{message}\"\n"
            )
        );
    }
}

/// A panic no operation matched for still reports, under the placeholder the
/// other events use, so its shape never varies.
#[test]
fn an_unmatched_panic_is_reported_under_the_placeholder() {
    let lines = captured(|| Observer::<()>::on_panic(&Trace::new(), &"boom", None));

    assert_eq!(
        lines,
        "ERROR kynos::middleware::trace: handler panicked matched_path=\"<unmatched>\" \
         operation_id=\"<unmatched>\" panic=\"boom\"\n"
    );
}

/// What a handler does inside the span: records `user_id` into the current
/// span, logs one event, and answers 201.
struct Handler;

impl ErasedTerminal<()> for Handler {
    fn call<'a>(
        &'a self,
        request: Request,
        context: &'a (),
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        let _ = (request, context);
        Box::pin(async {
            tracing::Span::current().record("user_id", "u-1");
            tracing::info!("handled");
            let mut response = Response::new(Body::empty());
            *response.status_mut() = StatusCode::CREATED;
            response
        })
    }
}

/// The chain `layers` and then [`Handler`] answer `request` with, matched to
/// `route`. Nothing in it waits, so one poll settles it.
fn served(
    layers: &[Arc<dyn ErasedInterceptor<()>>],
    route: Option<Route<'static>>,
    request: Request,
) -> Response {
    let future = Next::new(layers, &Handler, &(), route).run(request);
    match pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(continued) => continued.into_response(),
        Poll::Pending => panic!("nothing in the chain waits"),
    }
}

/// A `GET` request carrying `fields`.
fn request(fields: &[(&str, &str)]) -> Request {
    let mut request = Request::new(Body::empty());
    *request.headers_mut() = map(fields);
    request
}

/// The route the span tests match.
fn get_user() -> Route<'static> {
    Route::new("/users/{id}", "getUser", Method::Get)
}

/// The handler's event carries the span's opening fields, the one it recorded
/// itself among them, and the span closes with the status and latency.
#[test]
fn the_handlers_event_inherits_the_request_span() {
    let request_id = RequestId::new();
    let span = Trace::new()
        .correlating(&request_id)
        .span(&["user_id", "session_id"]);

    let lines = captured(|| {
        let response = served(
            &[Arc::new(request_id), Arc::new(span)],
            Some(get_user()),
            request(&[("x-request-id", "forged")]),
        );
        assert_eq!(response.status(), StatusCode::CREATED);
    });
    // `fmt` pads a level to five columns.
    let mut lines = lines.lines().map(str::trim_start);

    let handled = lines.next().expect("the handler's event");
    assert!(
        handled.starts_with(
            "INFO request{method=\"GET\" matched_path=\"/users/{id}\" \
             operation_id=\"getUser\" request_id=\"0\" user_id=\"u-1\"}: "
        ),
        "{handled}"
    );
    assert!(handled.ends_with(": handled"), "{handled}");

    let closed = lines.next().expect("the span's closing");
    assert!(closed.contains(" status=201 latency_ms="), "{closed}");
    assert!(closed.ends_with(": close"), "{closed}");
    assert_eq!(lines.next(), None);
}

/// Mounted inside `RequestId`, the span opens with the identifier it assigned
/// whatever group carries it, so a `Trace` never told the group still names it.
#[test]
fn a_span_inside_request_id_carries_the_assigned_identifier_unnamed() {
    let lines = captured(|| {
        served(
            &[
                Arc::new(RequestId::new().header::<Pair>()),
                Arc::new(Trace::new().span(&[])),
            ],
            Some(get_user()),
            request(&[]),
        );
    });

    assert!(lines.contains("request_id=\"0\""), "{lines}");
}

/// Mounted before `RequestId`, the span opens before an identifier is
/// assigned, so it carries what the client sent: the documented reason to
/// mount it after.
#[test]
fn a_span_outside_request_id_carries_what_the_client_sent() {
    let request_id = RequestId::new();
    let span = Trace::new().correlating(&request_id).span(&[]);

    let lines = captured(|| {
        served(
            &[Arc::new(span), Arc::new(request_id)],
            Some(get_user()),
            request(&[("x-request-id", "forged")]),
        );
    });

    assert!(lines.contains("request_id=\"forged\""), "{lines}");
}

/// A request nothing matched opens its span under the placeholder the events
/// use, so the span's shape never varies.
#[test]
fn an_unmatched_request_opens_its_span_under_the_placeholder() {
    let lines = captured(|| {
        served(&[Arc::new(Trace::new().span(&[]))], None, request(&[]));
    });

    assert!(
        lines.contains(
            "request{method=\"GET\" matched_path=\"<unmatched>\" \
             operation_id=\"<unmatched>\" request_id=\"\"}"
        ),
        "{lines}"
    );
}

/// `record_headers` reaches the span as it does the opening event, a
/// redacted header included; without it the span has no `headers` value.
#[test]
fn recorded_headers_open_the_span_only_when_asked_for() {
    let fields = [("authorization", "Bearer eyJ"), ("x-tenant", "acme")];
    let recorded = Trace::new()
        .record_headers(&["x-tenant", "authorization"])
        .span(&[]);

    let with = captured(|| {
        served(&[Arc::new(recorded)], Some(get_user()), request(&fields));
    });
    let without = captured(|| {
        served(
            &[Arc::new(Trace::new().span(&[]))],
            Some(get_user()),
            request(&fields),
        );
    });

    assert!(
        with.contains("headers=\"x-tenant=acme, authorization=<redacted>\""),
        "{with}"
    );
    assert!(!with.contains("eyJ"), "{with}");
    assert!(!without.contains("headers="), "{without}");
}

/// The span is at the level `Trace` sets, so a subscriber filtering that
/// level out leaves the handler's event outside any request span.
#[test]
fn the_span_takes_the_level_set_and_is_filtered_by_it() {
    let lines = captured(|| {
        served(
            &[Arc::new(Trace::new().level(Level::Warn).span(&[]))],
            Some(get_user()),
            request(&[]),
        );
    });
    let filtered = captured_at(tracing::Level::INFO, || {
        served(
            &[Arc::new(Trace::new().level(Level::Debug).span(&[]))],
            Some(get_user()),
            request(&[]),
        );
    });

    let closed = lines.lines().last().expect("the span's closing");
    assert!(closed.trim_start().starts_with("WARN request{"), "{closed}");
    assert_eq!(
        filtered.trim_start(),
        "INFO kynos::middleware::trace::tests: handled\n"
    );
}

/// The span's fields are its own, then the caller's in order, each once,
/// whatever the caller lists twice or shares with the span's own.
#[test]
fn a_declared_field_is_declared_once() {
    let callsite = SpanCallsite::interned(Level::Info, &["user_id", "status", "user_id", "tenant"]);
    let names: Vec<&str> = callsite
        .static_metadata()
        .fields()
        .iter()
        .map(|field| field.name())
        .collect();

    let mut expected = SPAN_FIELDS.to_vec();
    expected.extend(["user_id", "tenant"]);
    assert_eq!(names, expected);
}

/// One callsite per level and field list, so building a span again leaks
/// nothing more, and a different level or list gets its own.
#[test]
fn a_callsite_is_built_once_per_level_and_field_list() {
    let first = SpanCallsite::interned(Level::Info, &["user_id"]);
    let owned = vec!["user_id"];

    assert!(std::ptr::eq(
        first,
        SpanCallsite::interned(Level::Info, owned.leak())
    ));
    assert!(!std::ptr::eq(
        first,
        SpanCallsite::interned(Level::Debug, &["user_id"])
    ));
    assert!(!std::ptr::eq(
        first,
        SpanCallsite::interned(Level::Info, &["session_id"])
    ));
}

/// A group of two names, to tell the first declared from the first present.
struct Pair(HeaderValue);

impl HeaderParams for Pair {
    const NAMES: &'static [&'static str] = &["x-correlation-id", "x-trace-id"];
}

impl EncodeHeaders for Pair {
    fn encode(&self) -> Result<Vec<(crate::http::HeaderName, HeaderValue)>, UnencodableHeader> {
        Ok(Self::NAMES
            .iter()
            .map(|name| (crate::http::HeaderName::from_static(name), self.0.clone()))
            .collect())
    }
}

impl CorrelationHeaders for Pair {
    fn from_id(id: HeaderValue) -> Self {
        Self(id)
    }
}
