//! `Cors`'s reading of `Origin` on an actual request, and the router's
//! preflight answer over `Origin`, `Access-Control-Request-Method` and
//! `Access-Control-Request-Headers`, in `middleware/cors/`.
//!
//! The first octet's low two bits choose credentials and `allow_any_header`;
//! the rest is up to three lines, the three fields in that order, a missing
//! line being a missing field. The configuration permits one listed origin,
//! and every origin a suffix predicate accepts.
//!
//! An origin is permitted exactly when it is text that equals the listed one
//! in any ASCII case, or ends in the suffix. An actual response carries
//! `Access-Control-Allow-Origin` exactly then, echoing the field octet for
//! octet, and `Access-Control-Allow-Credentials: true` beside it exactly when
//! credentials are on.
//!
//! An `OPTIONS` carrying both `Origin` and a requested method is a preflight:
//! a 204 varying on all three fields, granting exactly when the origin is
//! permitted and the method, compared in any case, is one of the two the
//! `Cors` covers. A grant advertises those two, never the `DELETE` the path
//! serves under no `Cors`, and the request headers the configuration names,
//! echoing the requested ones only when credentials make `*` literal. Any other
//! `OPTIONS` is the router's own answer, carrying no CORS field.

#![no_main]

use kynos::{
    __private::fuzz,
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
};
use libfuzzer_sys::fuzz_target;

const LISTED: &str = "https://app.example";
const SUFFIX: &str = ".example.org";
const FIELDS: [HeaderName; 3] = [
    header::ORIGIN,
    header::ACCESS_CONTROL_REQUEST_METHOD,
    header::ACCESS_CONTROL_REQUEST_HEADERS,
];
const CORS_FIELDS: [HeaderName; 5] = [
    header::ACCESS_CONTROL_ALLOW_ORIGIN,
    header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
    header::ACCESS_CONTROL_ALLOW_METHODS,
    header::ACCESS_CONTROL_ALLOW_HEADERS,
    header::ACCESS_CONTROL_MAX_AGE,
];

fuzz_target!(|data: &[u8]| {
    let Some((&flags, rest)) = data.split_first() else {
        return;
    };
    let (credentials, any_header) = (flags & 1 != 0, flags & 2 != 0);

    let mut headers = HeaderMap::new();
    if !rest.is_empty() {
        for (name, line) in FIELDS.into_iter().zip(rest.split(|&byte| byte == b'\n')) {
            let Ok(value) = HeaderValue::from_bytes(line) else {
                return;
            };
            headers.insert(name, value);
        }
    }

    let (actual, status, preflight) =
        fuzz::cors(&[LISTED], SUFFIX, credentials, any_header, &headers);

    let origin = headers.get(header::ORIGIN);
    let permitted = origin
        .and_then(|origin| origin.to_str().ok())
        .is_some_and(|origin| origin.eq_ignore_ascii_case(LISTED) || origin.ends_with(SUFFIX));
    let granted = |fields: &HeaderMap| {
        assert_eq!(
            fields.get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            origin,
            "{headers:?}"
        );
        assert_eq!(
            fields
                .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
                .map(HeaderValue::as_bytes),
            credentials.then_some(&b"true"[..]),
            "{headers:?}"
        );
    };

    if permitted {
        granted(&actual);
    } else {
        assert!(actual.is_empty(), "{actual:?} for {headers:?}");
    }

    let Some(method) = headers
        .get(header::ACCESS_CONTROL_REQUEST_METHOD)
        .filter(|_| origin.is_some())
    else {
        assert_ne!(status, StatusCode::NO_CONTENT, "{headers:?}");
        for name in CORS_FIELDS {
            assert!(!preflight.contains_key(&name), "{name} for {headers:?}");
        }
        return;
    };

    assert_eq!(status, StatusCode::NO_CONTENT, "{headers:?}");
    let varies: Vec<String> = preflight
        .get_all(header::VARY)
        .iter()
        .flat_map(|value| value.to_str().expect("a written Vary").split(','))
        .map(|name| name.trim().to_ascii_lowercase())
        .collect();
    for name in FIELDS {
        assert!(
            varies.iter().any(|varied| varied == name.as_str()),
            "{varies:?}"
        );
    }

    let covered = ["GET", "POST"]
        .iter()
        .any(|covered| method.as_bytes().eq_ignore_ascii_case(covered.as_bytes()));
    if !(permitted && covered) {
        for name in CORS_FIELDS {
            assert!(!preflight.contains_key(&name), "{name} for {headers:?}");
        }
        return;
    }

    granted(&preflight);
    assert_eq!(
        preflight.get(header::ACCESS_CONTROL_ALLOW_METHODS),
        Some(&HeaderValue::from_static("GET, POST")),
        "{headers:?}"
    );
    let allowed = match (any_header, credentials) {
        (true, true) => headers.get(header::ACCESS_CONTROL_REQUEST_HEADERS).cloned(),
        (true, false) => Some(HeaderValue::from_static("*, authorization")),
        (false, _) => Some(HeaderValue::from_static("x-trace")),
    };
    assert_eq!(
        preflight.get(header::ACCESS_CONTROL_ALLOW_HEADERS),
        allowed.as_ref(),
        "{headers:?}"
    );
});
