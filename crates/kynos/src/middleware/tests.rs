use super::{Continued, EncodeHeaders};
use crate::{
    extract::params::header::HeaderParams,
    http::{HeaderName, HeaderValue, Response, header},
};

/// A group that declares no header of its own and varies on `origin` —
/// the shape `Cors` takes.
struct VariesOnOrigin;

impl HeaderParams for VariesOnOrigin {
    const NAMES: &'static [&'static str] = &[];
    const VARIES: &'static [&'static str] = &["origin"];
}

impl EncodeHeaders for VariesOnOrigin {
    fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
        Vec::new()
    }
}

#[test]
fn the_credentialed_wildcard_exposure_refusal_reads_as_one_sentence() {
    assert_eq!(
        super::MiddlewareError::CredentialedWildcardExposure.to_string(),
        "a CORS configuration exposes every response header and also permits credentials, which \
         the protocol reads as exposing a header literally named `*`; name the headers \
         `expose_headers` should expose, or drop `allow_credentials`"
    );
}

/// The `Vary` a response carries after `headers` rides on it.
fn vary_after<G: EncodeHeaders>(existing: Option<&str>, headers: G) -> Option<String> {
    let mut response = Response::new(crate::http::body::Body::empty());

    if let Some(existing) = existing {
        response.headers_mut().insert(
            header::VARY,
            HeaderValue::from_str(existing).expect("a representable Vary"),
        );
    }

    Continued::new(response)
        .with_headers(headers)
        .into_response()
        .headers()
        .get(header::VARY)
        .map(|value| value.to_str().expect("a printable Vary").to_owned())
}

/// The failure this exists to stop: `with_headers` used `insert`, so a
/// second contribution replaced the first rather than joining it — and a
/// response varying on two fields that advertised one is a cache poisoning
/// bug rather than a missing nicety.
#[test]
fn a_vary_union_keeps_the_field_names_already_present() {
    let vary = vary_after(Some("accept"), VariesOnOrigin).expect("a Vary");
    let names: Vec<_> = vary.split(',').map(str::trim).collect();

    assert!(names.contains(&"accept"), "lost the existing field: {vary}");
    assert!(names.contains(&"origin"), "never added its own: {vary}");
}

/// `Vary` is a set of field names, and RFC 9110 section 5.1 makes a field
/// name case-insensitive, so the same name in two spellings is one member.
#[test]
fn a_vary_union_adds_no_name_twice_whatever_its_case() {
    let vary = vary_after(Some("Origin"), VariesOnOrigin).expect("a Vary");
    let names: Vec<_> = vary.split(',').map(str::trim).collect();

    assert_eq!(names.len(), 1, "repeated one field name: {vary}");
}

/// `Vary: *` already says the response depends on more than the field names
/// can express, so adding one narrows nothing and must not appear to.
#[test]
fn a_wildcard_vary_absorbs_every_name_added_to_it() {
    let vary = vary_after(Some("*"), VariesOnOrigin).expect("a Vary");

    assert_eq!(vary, "*");
}

/// Every `Vary` line a response carries after `names` merge into `lines`.
fn vary_lines_after(lines: &[&[u8]], names: &'static [&'static str]) -> Vec<Vec<u8>> {
    let mut fields = crate::http::HeaderMap::new();

    for line in lines {
        fields.append(
            header::VARY,
            HeaderValue::from_bytes(line).expect("a representable Vary line"),
        );
    }

    super::vary_on(&mut fields, names);

    fields
        .get_all(header::VARY)
        .iter()
        .map(|value| value.as_bytes().to_owned())
        .collect()
}

/// RFC 9110 section 5.3 lets a list field arrive split across lines, so a
/// name on the second line is as much a member as one on the first.
/// Reading only the first and then replacing every line dropped `cookie`
/// here, and a cache keyed on what was left served one user's response to
/// another.
#[test]
fn a_vary_union_keeps_the_field_names_of_every_line() {
    let lines = vary_lines_after(&[b"accept-language", b"cookie"], &["accept-encoding"]);

    assert_eq!(
        lines,
        [b"accept-language, cookie, accept-encoding".to_vec()]
    );
}

/// A wildcard on a later line absorbs every name just as one on the first
/// does.
#[test]
fn a_wildcard_on_any_vary_line_absorbs_every_name_added_to_it() {
    let lines = vary_lines_after(&[b"accept", b"*"], &["origin"]);

    assert_eq!(lines, [b"accept".to_vec(), b"*".to_vec()]);
}

/// Lines that already hold every name stay as they were: the merge rewrites
/// the field only when it has a name to add.
#[test]
fn vary_lines_that_already_hold_every_name_are_left_as_they_were() {
    let lines = vary_lines_after(&[b"origin", b"cookie"], &["origin"]);

    assert_eq!(lines, [b"origin".to_vec(), b"cookie".to_vec()]);
}

/// A line that is not UTF-8 still names something a cache must key on, so
/// it survives byte for byte, and so does every line beside it.
#[test]
fn a_vary_line_that_is_not_utf8_is_kept_with_every_other_line() {
    let lines = vary_lines_after(&[b"x-caf\xe9", b"cookie"], &["origin"]);

    assert_eq!(lines, [b"x-caf\xe9, cookie, origin".to_vec()]);
}

/// A group that is not repeatable replaces whatever was there.
///
/// The control for the repeatable case, which
/// `response::headers::tests::a_group_writes_the_same_fields_whichever_path_it_reaches_the_wire_by`
/// pins on this path and the handler's alike. Without it "repeatable appends" would read as "everything
/// appends", and a second `Content-Encoding` beside a first is a response
/// no client can decode.
#[test]
fn a_group_that_is_not_repeatable_replaces_the_value_already_set() {
    struct OneEncoding;

    impl HeaderParams for OneEncoding {
        const NAMES: &'static [&'static str] = &["content-encoding"];
    }

    impl EncodeHeaders for OneEncoding {
        fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
            vec![(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"))]
        }
    }

    let mut response = Response::new(crate::http::body::Body::empty());
    response
        .headers_mut()
        .insert(header::CONTENT_ENCODING, HeaderValue::from_static("br"));

    let sent: Vec<_> = Continued::new(response)
        .with_headers(OneEncoding)
        .into_response()
        .headers()
        .get_all(header::CONTENT_ENCODING)
        .iter()
        .map(|value| value.to_str().expect("a printable field").to_owned())
        .collect();

    assert_eq!(sent, ["gzip"]);
}

/// A group varying on nothing leaves the header absent rather than empty.
#[test]
fn a_group_that_varies_on_nothing_writes_no_vary() {
    struct Silent;

    impl HeaderParams for Silent {
        const NAMES: &'static [&'static str] = &[];
    }

    impl EncodeHeaders for Silent {
        fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
            Vec::new()
        }
    }

    assert_eq!(vary_after(None, Silent), None);
}

/// A group encoding a field its `NAMES` never declared fails the build.
///
/// `NAMES` is what `CompatibleWith` compares, so a group declaring one field
/// and writing another puts a header on the wire the conflict check cannot
/// see -- the same escape `with_headers` allowed, reached from inside a group
/// rather than from a second call. A derived group cannot drift this way; a
/// hand-written pair can, and fifteen of them ship.
///
/// Debug only, because the guard is a `debug_assert`: the response path does
/// not panic in release, for the reason `vary_on` gives.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "which its `NAMES` does not declare")]
fn a_group_encoding_an_undeclared_field_is_refused() {
    struct Lying;

    impl HeaderParams for Lying {
        const NAMES: &'static [&'static str] = &["x-declared"];
    }

    impl EncodeHeaders for Lying {
        fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
            vec![(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"))]
        }
    }

    let _ = Continued::new(Response::new(crate::http::body::Body::empty())).with_headers(Lying);
}

/// The control: a group encoding exactly what it declared is written.
///
/// Without it the case above would pass on any panic at all, including one
/// from a `Continued` that had stopped working entirely.
#[test]
fn a_group_encoding_what_it_declared_is_written() {
    struct Honest;

    impl HeaderParams for Honest {
        const NAMES: &'static [&'static str] = &["content-encoding"];
    }

    impl EncodeHeaders for Honest {
        fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
            vec![(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"))]
        }
    }

    let response = Continued::new(Response::new(crate::http::body::Body::empty()))
        .with_headers(Honest)
        .into_response();

    assert_eq!(
        response.headers().get(header::CONTENT_ENCODING),
        Some(&HeaderValue::from_static("gzip"))
    );
}

/// Removing a field the group never declared fails the build.
///
/// `remove_declared` is held to the rule `with_headers` writes under, so an
/// interceptor cannot strip a field some other group owns behind a name its
/// own `NAMES` never stated. Debug only, for the reason the case above gives.
#[cfg(all(debug_assertions, feature = "compression"))]
#[test]
#[should_panic(expected = "which its `NAMES` does not declare")]
fn a_group_removing_an_undeclared_field_is_refused() {
    struct DeclaresEncoding;

    impl HeaderParams for DeclaresEncoding {
        const NAMES: &'static [&'static str] = &["content-encoding"];
    }

    let mut continued = Continued::new(Response::new(crate::http::body::Body::empty()));

    continued.remove_declared::<DeclaresEncoding>(&header::CONTENT_LENGTH);
}

/// The control: removing a field the group declared takes it off the response.
///
/// Without it the case above would pass on any panic at all, and a removal
/// that did nothing would go unnoticed.
#[cfg(feature = "compression")]
#[test]
fn a_group_removing_a_declared_field_removes_it() {
    struct DeclaresLength;

    impl HeaderParams for DeclaresLength {
        const NAMES: &'static [&'static str] = &["content-length"];
    }

    let mut response = Response::new(crate::http::body::Body::empty());
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, HeaderValue::from_static("5"));
    let mut continued = Continued::new(response);

    continued.remove_declared::<DeclaresLength>(&header::CONTENT_LENGTH);

    assert_eq!(
        continued
            .into_response()
            .headers()
            .get(header::CONTENT_LENGTH),
        None
    );
}

mod security_headers {
    use std::{
        net::{IpAddr, SocketAddr},
        time::Duration,
    };

    use kynos_openapi::RefOr;

    use crate::{
        extract::{
            connection::{Connection, TlsIdentity},
            params::header::{EncodeHeaders, HeaderParams},
        },
        http::{
            HeaderMap, HeaderValue, Response,
            forwarded::{Forwarded, ProxyHeader, TrustedProxies},
            header,
        },
        middleware::{
            Continued,
            security_headers::{SecurityFields, StrictTransportSecurity, conveyed_securely},
        },
        schema::registry::Registry,
    };

    const PEER: &str = "192.0.2.1:50000";
    const LOCAL: &str = "192.0.2.2:443";
    const PROXY: &str = "10.0.0.1";

    fn plain() -> Connection {
        Connection::from_peer(PEER.parse().unwrap(), LOCAL.parse().unwrap())
    }

    fn tls() -> Connection {
        Connection::from_tls_peer(
            PEER.parse().unwrap(),
            LOCAL.parse().unwrap(),
            TlsIdentity::default(),
        )
    }

    /// What the router resolves for a request from `peer`, believing a proxy
    /// at `PROXY` that writes the `X-Forwarded` pair.
    fn behind_proxy(peer: SocketAddr, fields: &[(&'static str, &'static str)]) -> Forwarded {
        let mut headers = HeaderMap::new();
        for (name, value) in fields {
            headers.insert(*name, HeaderValue::from_static(value));
        }

        let trusted =
            TrustedProxies::addresses(ProxyHeader::XForwarded, [PROXY.parse::<IpAddr>().unwrap()]);

        Forwarded::resolve(&headers, Some(peer), &trusted)
    }

    fn group<const H: bool, const F: bool>(
        transport: Option<&'static str>,
    ) -> SecurityFields<H, F> {
        SecurityFields {
            transport: transport.map(HeaderValue::from_static),
        }
    }

    /// The fields a group leaves on a response that already said
    /// `Cache-Control: max-age=60`.
    fn written<const H: bool, const F: bool>(group: SecurityFields<H, F>) -> HeaderMap {
        let mut response = Response::new(crate::http::body::Body::empty());
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("max-age=60"),
        );

        Continued::new(response)
            .with_headers(group)
            .into_response()
            .headers()
            .clone()
    }

    /// The `const` documented is the value sent only because the field is
    /// replaced: a handler's own `max-age` surviving beside the description's
    /// `no-store` would be a description the wire contradicts.
    #[test]
    fn the_baseline_replaces_what_the_chain_set_and_opts_into_nothing() {
        let sent = written(group::<false, false>(None));

        assert_eq!(
            sent.get_all(header::CACHE_CONTROL)
                .iter()
                .collect::<Vec<_>>(),
            ["no-store"]
        );
        assert_eq!(sent[header::REFERRER_POLICY], "no-referrer");
        assert_eq!(sent[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert!(!sent.contains_key(header::STRICT_TRANSPORT_SECURITY));
        assert!(!sent.contains_key(header::X_FRAME_OPTIONS));
    }

    /// Every opt-in combination writes exactly the names it declares, so the
    /// conflict check compares what reaches the wire.
    #[test]
    fn each_combination_writes_exactly_what_it_declares() {
        fn written<G: EncodeHeaders>(group: &G) -> Vec<String> {
            let mut names: Vec<_> = group
                .encode()
                .into_iter()
                .map(|(name, _)| name.as_str().to_owned())
                .collect();
            names.sort();
            names
        }

        fn declared<G: HeaderParams>() -> Vec<String> {
            let mut names: Vec<_> = G::NAMES.iter().map(|&name| name.to_owned()).collect();
            names.sort();
            names
        }

        let hsts = Some("max-age=1");

        assert_eq!(
            written(&group::<false, false>(None)),
            declared::<SecurityFields<false, false>>()
        );
        assert_eq!(
            written(&group::<true, false>(hsts)),
            declared::<SecurityFields<true, false>>()
        );
        assert_eq!(
            written(&group::<false, true>(None)),
            declared::<SecurityFields<false, true>>()
        );
        assert_eq!(
            written(&group::<true, true>(hsts)),
            declared::<SecurityFields<true, true>>()
        );
    }

    /// RFC 6797 section 7.2: the declared field is withheld where the
    /// transport was not secure, a subset of what is declared.
    #[test]
    fn hsts_is_left_off_where_the_transport_was_not_secure() {
        let sent = written(group::<true, true>(None));

        assert!(!sent.contains_key(header::STRICT_TRANSPORT_SECURITY));
        assert_eq!(sent[header::X_FRAME_OPTIONS], "DENY");
    }

    #[test]
    fn an_hsts_policy_renders_whole_seconds_and_its_directive() {
        assert_eq!(
            StrictTransportSecurity::max_age(Duration::from_secs(31_536_000)).value(),
            "max-age=31536000"
        );
        assert_eq!(
            StrictTransportSecurity::max_age(Duration::from_millis(1_999))
                .include_subdomains()
                .value(),
            "max-age=1; includeSubDomains"
        );
        assert_eq!(
            StrictTransportSecurity::max_age(Duration::ZERO).value(),
            "max-age=0"
        );
    }

    /// The decision RFC 6797 section 7.2 turns on: a trusted hop's scheme
    /// first, the socket's TLS only where the socket peer is the client, and
    /// nothing for a request no socket carried.
    #[test]
    fn a_secure_transport_is_the_clients_own() {
        let peer: SocketAddr = PEER.parse().unwrap();
        let from_proxy: SocketAddr = format!("{PROXY}:50000").parse().unwrap();
        let direct = Forwarded::resolve(&HeaderMap::new(), Some(peer), &TrustedProxies::none());

        let cases = [
            ("no socket", None, None, false),
            ("plain socket", None, Some(plain()), false),
            ("TLS socket", None, Some(tls()), true),
            (
                "TLS socket, nobody trusted",
                Some(direct.clone()),
                Some(tls()),
                true,
            ),
            (
                "plain socket, nobody trusted",
                Some(direct),
                Some(plain()),
                false,
            ),
            (
                "a trusted hop says https over a plain socket",
                Some(behind_proxy(
                    from_proxy,
                    &[
                        ("x-forwarded-for", "203.0.113.9"),
                        ("x-forwarded-proto", "https"),
                    ],
                )),
                Some(plain()),
                true,
            ),
            (
                "a trusted hop says http over a TLS socket",
                Some(behind_proxy(
                    from_proxy,
                    &[
                        ("x-forwarded-for", "203.0.113.9"),
                        ("x-forwarded-proto", "http"),
                    ],
                )),
                Some(tls()),
                false,
            ),
            (
                "a trusted hop names a client and no scheme over a TLS socket",
                Some(behind_proxy(
                    from_proxy,
                    &[("x-forwarded-for", "203.0.113.9")],
                )),
                Some(tls()),
                false,
            ),
        ];

        for (case, forwarded, connection, secure) in cases {
            assert_eq!(
                conveyed_securely(forwarded.as_ref(), connection.as_ref()),
                secure,
                "{case}"
            );
        }
    }

    /// Each fixed field is described as the one value it carries, and as
    /// always present; HSTS, withheld over plain transport, is not required.
    #[test]
    fn each_fixed_field_is_described_as_its_constant() {
        let headers = SecurityFields::<true, true>::response_headers(&mut Registry::new());

        let described = |name: &str| match headers.get(name) {
            Some(RefOr::Item(header)) => serde_json::to_value(header).unwrap(),
            other => panic!("{name} is not an inline header: {other:?}"),
        };

        for (name, value) in [
            ("Cache-Control", "no-store"),
            ("Referrer-Policy", "no-referrer"),
            ("X-Content-Type-Options", "nosniff"),
            ("X-Frame-Options", "DENY"),
        ] {
            let header = described(name);
            let schema = &header["content"]["text/plain"]["schema"];
            assert_eq!(header["required"], true, "{name}");
            assert_eq!(schema["type"], "string", "{name}");
            assert_eq!(schema["const"], value, "{name}");
        }

        let hsts = described("Strict-Transport-Security");
        assert_eq!(hsts.get("required"), None);
        assert_eq!(hsts["content"]["text/plain"]["schema"]["type"], "string");
        assert_eq!(headers.len(), 5);
    }

    #[test]
    fn the_baseline_describes_no_opt_in() {
        let headers = SecurityFields::<false, false>::response_headers(&mut Registry::new());

        let mut names: Vec<_> = headers.keys().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            ["Cache-Control", "Referrer-Policy", "X-Content-Type-Options"]
        );
    }
}
