use ::http::uri::Authority;

use super::{TrustedHosts, canonical, is_loopback};
use crate::http::{HeaderMap, HeaderValue, header};

/// `admits` over a request naming `host` in `Host` alone.
fn admits_host(hosts: &TrustedHosts, host: &str) -> bool {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::HOST,
        HeaderValue::from_str(host).expect("a printable field"),
    );
    hosts.admits(&headers, None)
}

/// Every host a configuration naming one host and loopback decides, and what
/// it decides.
///
/// One table: whether a host is admitted is the same question over every
/// spelling of it, and a table shows which spellings the rules treat alike.
#[test]
fn every_spelling_of_a_host_is_decided_as_the_host_is() {
    let hosts = TrustedHosts::new()
        .allow("api.example.com")
        .allow_loopback();

    for (host, expected) in [
        ("api.example.com", true),
        // The port is not compared: a rebinding page names the one the
        // service listens on.
        ("api.example.com:8080", true),
        ("API.Example.COM", true),
        ("api.example.com.", true),
        ("api.example.com.:443", true),
        // The attack itself: the attacker's own name, rebound.
        ("attacker.example", false),
        ("attacker.example:8080", false),
        // No wildcard, in either direction.
        ("evil.api.example.com", false),
        ("example.com", false),
        ("api.example.com.evil.example", false),
        // Loopback, by name and by address.
        ("localhost", true),
        ("localhost:3000", true),
        ("LOCALHOST", true),
        ("localhost.", true),
        ("app.localhost", true),
        ("127.0.0.1", true),
        ("127.0.0.1:3000", true),
        ("127.1.2.3", true),
        ("[::1]", true),
        ("[::1]:3000", true),
        ("[0:0:0:0:0:0:0:1]", true),
        ("[::ffff:127.0.0.1]", true),
        // Not loopback, though it may reach a service bound to every address.
        ("0.0.0.0", false),
        ("[::]", false),
        ("10.0.0.1", false),
        ("localhost.example", false),
        ("notlocalhost", false),
        // Not an authority at all.
        ("", false),
        ("a b", false),
    ] {
        assert_eq!(admits_host(&hosts, host), expected, "{host:?}");
    }
}

/// Without `allow_loopback`, a loopback host is admitted only where listed.
#[test]
fn loopback_is_admitted_only_when_asked_for() {
    let hosts = TrustedHosts::new().allow("api.example.com");

    for host in ["localhost", "127.0.0.1", "[::1]"] {
        assert!(!admits_host(&hosts, host), "{host}");
    }
    assert!(admits_host(
        &TrustedHosts::new().allow("localhost"),
        "localhost"
    ));
}

/// An allowed host is canonicalised as a request's is, so it matches however
/// either side spells it.
#[test]
fn an_allowed_host_is_matched_however_it_is_spelt() {
    for (allowed, requested) in [
        ("API.example.com", "api.example.com"),
        ("api.example.com.", "api.example.com"),
        ("::1", "[::1]"),
        ("[0::1]", "[::1]:8080"),
        ("[::1]", "[0:0:0:0:0:0:0:1]"),
        ("192.0.2.1", "192.0.2.1:80"),
    ] {
        assert!(
            admits_host(&TrustedHosts::new().allow(allowed), requested),
            "{allowed} did not admit {requested}"
        );
    }
}

/// A `TrustedHosts` with nothing allowed admits nothing, and a request
/// stating no authority is admitted by nothing.
#[test]
fn nothing_is_admitted_without_a_host_on_both_sides() {
    assert!(!admits_host(&TrustedHosts::new(), "api.example.com"));
    assert!(!admits_host(&TrustedHosts::new(), "localhost"));

    let hosts = TrustedHosts::new()
        .allow("api.example.com")
        .allow_loopback();
    assert!(!hosts.admits(&HeaderMap::new(), None));
}

/// The target's authority decides where a request states both, as RFC 9112
/// section 3.2.2 and RFC 9113 section 8.3.1 have it.
#[test]
fn the_target_authority_decides_where_a_request_states_both() {
    let hosts = TrustedHosts::new().allow("api.example.com");
    let target = |authority: &'static str| Authority::from_static(authority);
    let with_host = |host: &'static str| {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static(host));
        headers
    };

    assert!(hosts.admits(
        &with_host("attacker.example"),
        Some(&target("api.example.com"))
    ));
    assert!(
        !hosts.admits(
            &with_host("api.example.com"),
            Some(&target("attacker.example"))
        ),
        "a `Host` the target outranks admitted the request"
    );
    assert!(hosts.admits(&HeaderMap::new(), Some(&target("user@api.example.com:1"))));
}

/// What a host canonicalises to, and that an already-canonical one is
/// borrowed rather than copied.
#[test]
fn a_host_canonicalises_to_one_spelling() {
    for (host, expected) in [
        ("api.example.com", Some("api.example.com")),
        ("Api.Example.Com", Some("api.example.com")),
        ("api.example.com.", Some("api.example.com")),
        ("::1", Some("[::1]")),
        ("[::1]", Some("[::1]")),
        ("[0:0::1]", Some("[::1]")),
        ("127.0.0.1", Some("127.0.0.1")),
        ("", None),
        (".", None),
    ] {
        assert_eq!(canonical(host).as_deref(), expected, "{host:?}");
    }

    assert!(matches!(
        canonical("api.example.com"),
        Some(std::borrow::Cow::Borrowed(_))
    ));
    assert!(matches!(
        canonical("[::1]"),
        Some(std::borrow::Cow::Borrowed(_))
    ));
}

/// What counts as loopback, over canonical hosts.
#[test]
fn loopback_is_localhost_and_the_loopback_ranges() {
    for host in [
        "localhost",
        "a.b.localhost",
        "127.0.0.1",
        "127.255.255.255",
        "[::1]",
        "[::ffff:127.0.0.1]",
    ] {
        assert!(is_loopback(host), "{host}");
    }
    for host in [
        "0.0.0.0",
        "128.0.0.1",
        "[::]",
        "[::2]",
        "[::ffff:10.0.0.1]",
        "localhostx",
        "xlocalhost",
        "localhost.example",
    ] {
        assert!(!is_loopback(host), "{host}");
    }
}

/// What `TrustedHosts` answers on the wire.
mod wire {
    use http_body_util::BodyExt;

    use crate::{
        Router,
        extract::body::text::Text,
        http::{Request, StatusCode, body::Body, header},
        middleware::trusted_hosts::TrustedHosts,
        openapi::{Method, PathTemplate},
        router::{endpoint::builder::EndpointBuilder, service::Service},
    };

    async fn read() -> Text {
        Text("served".to_owned())
    }

    fn service() -> Service<()> {
        let endpoint = EndpointBuilder::new(
            Method::Get,
            PathTemplate::parse("/status").expect("a valid path"),
            read,
        );
        Router::<()>::new()
            .mount(endpoint)
            .intercept(TrustedHosts::new().allow_loopback())
            .build(())
            .expect("a describable router")
    }

    /// A same-origin `GET` from a page whose name was rebound to loopback.
    fn for_host(host: &str) -> Request {
        http::Request::builder()
            .uri("/status")
            .header(header::HOST, host)
            .header(header::ORIGIN, format!("http://{host}"))
            .header("sec-fetch-site", "same-origin")
            .body(Body::empty())
            .expect("a well-formed request")
    }

    /// A request for a loopback name reaches the handler.
    #[tokio::test]
    async fn a_trusted_host_is_served() {
        let response = service().call(for_host("localhost:8080")).await;

        assert_eq!(response.status(), StatusCode::OK);
    }

    /// A rebound request is refused with the 421 problem `TrustedHosts`
    /// declares, before the handler runs: a safe method is no exception, since
    /// reading is what a rebinding page wants.
    #[tokio::test]
    async fn a_rebound_request_is_refused_as_misdirected() {
        let response = service().call(for_host("attacker.example:8080")).await;

        assert_eq!(response.status(), StatusCode::MISDIRECTED_REQUEST);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .map(crate::http::HeaderValue::as_bytes),
            Some(b"application/problem+json".as_slice())
        );
        let body = response
            .into_body()
            .collect()
            .await
            .expect("a complete body")
            .to_bytes();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).expect("a problem document"),
            serde_json::json!({
                "type": "about:blank",
                "title": "Misdirected Request",
                "status": 421,
                "detail": "this server does not answer for the requested host",
            })
        );
    }

    /// The 421 is declared on every operation the interceptor covers.
    #[test]
    fn the_421_is_declared() {
        let document = serde_json::to_value(service().openapi()).expect("a serializable document");

        assert!(
            document["paths"]["/status"]["get"]["responses"]["421"].is_object(),
            "{document}"
        );
    }
}
