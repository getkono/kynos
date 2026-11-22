use ::http::uri::Authority;

use super::{TrustedHosts, UntrustedHostReason, canonical, is_loopback};
use crate::http::{HeaderMap, HeaderValue, header};

/// Whether `hosts` admits a request carrying `headers` whose target states
/// `authority`.
fn admits(hosts: &TrustedHosts, headers: &HeaderMap, authority: Option<&Authority>) -> bool {
    hosts.decides(headers, authority).is_ok()
}

/// `decides` over a request naming `host` in `Host` alone, or stating no
/// `Host` where it is `None`.
fn decision(hosts: &TrustedHosts, host: Option<&str>) -> Result<(), UntrustedHostReason> {
    let mut headers = HeaderMap::new();
    if let Some(host) = host {
        headers.insert(
            header::HOST,
            HeaderValue::from_str(host).expect("a printable field"),
        );
    }
    hosts.decides(&headers, None)
}

/// Whether [`decision`] admits a request naming `host` in `Host`.
fn admits_host(hosts: &TrustedHosts, host: &str) -> bool {
    decision(hosts, Some(host)).is_ok()
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
    assert!(!admits(&hosts, &HeaderMap::new(), None));
}

/// Why each refusal was made, for a request naming its authority in `Host`
/// alone or on its target.
///
/// One table: the reasons are the steps of one reading, and a table shows
/// which step each request falls at.
#[test]
fn every_refusal_names_its_reason() {
    let hosts = TrustedHosts::new()
        .allow("api.example.com")
        .allow_loopback();

    for (host, expected) in [
        (None, UntrustedHostReason::NoAuthority),
        // RFC 9112 section 3.2: the `Host` a client sends for a target with
        // no authority.
        (Some(""), UntrustedHostReason::NoAuthority),
        (Some("a b"), UntrustedHostReason::Malformed),
        (Some("[::1"), UntrustedHostReason::Malformed),
        (Some("."), UntrustedHostReason::Malformed),
        (Some("attacker.example"), UntrustedHostReason::NotAllowed),
        (Some("0.0.0.0:8080"), UntrustedHostReason::NotAllowed),
    ] {
        assert_eq!(decision(&hosts, host), Err(expected), "{host:?}");
    }

    assert_eq!(
        hosts.decides(
            &HeaderMap::new(),
            Some(&Authority::from_static("attacker.example"))
        ),
        Err(UntrustedHostReason::NotAllowed)
    );
}

/// `admit_no_authority` admits exactly the requests refused as stating no
/// authority, and a request naming a host is decided as before.
#[test]
fn admitting_no_authority_admits_only_a_request_without_one() {
    let hosts = TrustedHosts::new()
        .allow("api.example.com")
        .admit_no_authority();

    assert_eq!(decision(&hosts, None), Ok(()));
    assert_eq!(decision(&hosts, Some("")), Ok(()));
    assert_eq!(decision(&hosts, Some("api.example.com")), Ok(()));
    assert_eq!(
        decision(&hosts, Some("attacker.example")),
        Err(UntrustedHostReason::NotAllowed)
    );
    assert_eq!(
        decision(&hosts, Some("a b")),
        Err(UntrustedHostReason::Malformed)
    );
    assert_eq!(
        hosts.decides(
            &HeaderMap::new(),
            Some(&Authority::from_static("attacker.example"))
        ),
        Err(UntrustedHostReason::NotAllowed),
        "a target authority is one, whatever `Host` says"
    );
}

/// Every reason's label, closed over the variants: a reason added without one
/// fails to compile here.
///
/// The labels are a metric's values, so they are fixed text rather than
/// whatever `Debug` happens to print.
#[test]
fn every_reason_has_a_stable_snake_case_label() {
    let label = |reason: UntrustedHostReason| match reason {
        UntrustedHostReason::NoAuthority => "no_authority",
        UntrustedHostReason::Malformed => "malformed",
        UntrustedHostReason::NotAllowed => "not_allowed",
    };

    for reason in [
        UntrustedHostReason::NoAuthority,
        UntrustedHostReason::Malformed,
        UntrustedHostReason::NotAllowed,
    ] {
        assert_eq!(reason.as_str(), label(reason), "{reason:?}");
    }
}

/// `check` and `admits` read the target's authority and `Host` off a request
/// head, as the interceptor reads them off the request.
#[test]
fn check_decides_over_a_request_head() {
    let hosts = TrustedHosts::new().allow("api.example.com");
    let head = |uri: &str, host: Option<&str>| {
        let mut request = http::Request::builder().uri(uri);
        if let Some(host) = host {
            request = request.header(header::HOST, host);
        }
        request
            .body(())
            .expect("a well-formed request")
            .into_parts()
            .0
    };

    // The target's authority, not `Host`, is what is held to the list.
    assert_eq!(
        hosts.check(&head("http://api.example.com/", Some("attacker.example"))),
        Ok(())
    );
    assert_eq!(
        hosts.check(&head("http://attacker.example/", Some("api.example.com"))),
        Err(UntrustedHostReason::NotAllowed)
    );
    // `Host` is read where the target names no authority.
    assert_eq!(hosts.check(&head("/", Some("api.example.com"))), Ok(()));
    assert_eq!(
        hosts.check(&head("/", None)),
        Err(UntrustedHostReason::NoAuthority)
    );
    // `admits` is the same decision.
    assert!(hosts.admits(&head("/", Some("api.example.com"))));
    assert!(!hosts.admits(&head("/", Some("attacker.example"))));
}

/// What `TrustedHostsWhen` prints: the `TrustedHosts` it applies, and not the
/// predicate.
#[test]
fn a_trusted_hosts_when_prints_the_trusted_hosts_it_applies() {
    let hosts = TrustedHosts::new()
        .allow("api.example.com")
        .when(|enabled: &bool| *enabled);

    assert_eq!(
        format!("{:?}", hosts.clone()),
        r#"TrustedHostsWhen { hosts: TrustedHosts { hosts: ["api.example.com"], loopback: false, no_authority: false }, .. }"#
    );
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

    assert!(admits(
        &hosts,
        &with_host("attacker.example"),
        Some(&target("api.example.com"))
    ));
    assert!(
        !admits(
            &hosts,
            &with_host("api.example.com"),
            Some(&target("attacker.example"))
        ),
        "a `Host` the target outranks admitted the request"
    );
    assert!(admits(
        &hosts,
        &HeaderMap::new(),
        Some(&target("user@api.example.com:1"))
    ));
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

    /// The posture a [`TrustedHosts::when`] predicate reads.
    struct Posture {
        exposed: bool,
    }

    /// A service checking hosts only where `exposed` says so.
    fn conditional(exposed: bool) -> Service<Posture> {
        let endpoint = EndpointBuilder::new(
            Method::Get,
            PathTemplate::parse("/status").expect("a valid path"),
            read,
        );
        Router::<Posture>::new()
            .mount(endpoint)
            .intercept(
                TrustedHosts::new()
                    .allow_loopback()
                    .when(|posture: &Posture| posture.exposed),
            )
            .build(Posture { exposed })
            .expect("a describable router")
    }

    /// Enabled by the context, it refuses what `TrustedHosts` refuses;
    /// disabled, it lets the same request through.
    #[tokio::test]
    async fn trusted_hosts_when_decides_only_where_the_context_enables_it() {
        let enabled = conditional(true)
            .call(for_host("attacker.example:8080"))
            .await;
        assert_eq!(enabled.status(), StatusCode::MISDIRECTED_REQUEST);

        let disabled = conditional(false)
            .call(for_host("attacker.example:8080"))
            .await;
        assert_eq!(disabled.status(), StatusCode::OK);
    }

    /// The 421 is declared whether or not the context enables the check: the
    /// document describes what the operation may answer in any deployment.
    #[test]
    fn trusted_hosts_when_declares_its_421_either_way() {
        for exposed in [true, false] {
            let document = serde_json::to_value(conditional(exposed).openapi())
                .expect("a serializable document");

            assert!(
                document["paths"]["/status"]["get"]["responses"]["421"].is_object(),
                "{exposed}: {document}"
            );
        }
    }
}
