use super::{DEFAULT_CORRELATION, REDACTED, Trace};
use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams},
    http::{HeaderMap, HeaderValue},
    middleware::{
        Observer,
        request_id::{CorrelationHeaders, RequestId, XRequestId},
    },
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

    assert_eq!(Trace::new().inbound(&inbound), "");
    assert_eq!(
        Trace::new()
            .correlating(&RequestId::new().trust_client(false))
            .inbound(&inbound),
        ""
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
        "second"
    );
    assert_eq!(
        trusting.inbound(&map(&[
            ("x-trace-id", "second"),
            ("x-correlation-id", "first")
        ])),
        "first"
    );
    assert_eq!(trusting.inbound(&map(&[])), "");
}

/// The closing event carries what the response does, trusted or not.
#[test]
fn the_assigned_identifier_is_logged_on_departure() {
    let response = map(&[("x-request-id", "minted")]);

    assert_eq!(Trace::new().assigned(&response), "minted");
}

/// The lines a `fmt` subscriber writes while `emitting` runs on this thread.
fn captured(emitting: impl FnOnce()) -> String {
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
        .with_max_level(tracing::Level::TRACE)
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
        .level(tracing::Level::DEBUG)
        .correlating(&RequestId::new().trust_client(true));
    let mut request = crate::http::Request::new(crate::http::body::Body::empty());
    *request.headers_mut() = map(&[("x-request-id", "echoed")]);

    let lines = captured(|| Observer::<()>::on_request(&trace, &request, None, &()));

    assert!(lines.contains("DEBUG"), "{lines}");
    assert!(lines.contains("request received"), "{lines}");
    assert!(lines.contains("request_id=\"echoed\""), "{lines}");
}

/// The closing event carries the identifier the response does, and its status.
#[test]
fn the_closing_event_carries_the_assigned_identifier() {
    let mut response = crate::http::Response::new(crate::http::body::Body::empty());
    *response.headers_mut() = map(&[("x-request-id", "minted")]);

    let lines = captured(|| {
        Observer::<()>::on_response(&Trace::new(), &response, None, std::time::Duration::ZERO);
    });

    assert!(lines.contains("INFO"), "{lines}");
    assert!(lines.contains("response sent"), "{lines}");
    assert!(lines.contains("status=200"), "{lines}");
    assert!(lines.contains("request_id=\"minted\""), "{lines}");
}

/// A group of two names, to tell the first declared from the first present.
struct Pair(HeaderValue);

impl HeaderParams for Pair {
    const NAMES: &'static [&'static str] = &["x-correlation-id", "x-trace-id"];
}

impl EncodeHeaders for Pair {
    fn encode(&self) -> Vec<(crate::http::HeaderName, HeaderValue)> {
        Self::NAMES
            .iter()
            .map(|name| (crate::http::HeaderName::from_static(name), self.0.clone()))
            .collect()
    }
}

impl CorrelationHeaders for Pair {
    fn from_id(id: HeaderValue) -> Self {
        Self(id)
    }
}
