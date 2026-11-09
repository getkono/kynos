//! `Csrf::permits`, over `Sec-Fetch-Site`, `Origin`, `Host` and the request
//! target's authority, in `middleware/csrf.rs`.
//!
//! The first octet chooses the method from its low three bits, and with its
//! fourth whether the target is in absolute form. The rest is up to four
//! lines, the target's authority then `Host`, `Origin` and `Sec-Fetch-Site`, a
//! missing line being a missing field. The configuration trusts one origin.
//!
//! The decision is the one the type's documentation lists, transcribed rule
//! by rule: a safe method passes; then a trusted origin, compared in any case
//! and around whitespace; then a readable `Sec-Fetch-Site` decides alone,
//! passing only `same-origin` and `none`; then a readable `Origin` passes
//! exactly when what follows its scheme names the request's own authority,
//! the target's before `Host`'s, in any case; and a request with neither
//! readable field passes.

#![no_main]

use kynos::{
    http::{HeaderMap, HeaderName, HeaderValue, Method, Request, Uri, body::Body, header},
    middleware::csrf::Csrf,
};
use libfuzzer_sys::fuzz_target;

const TRUSTED: &str = "https://admin.example";
const METHODS: [Method; 8] = [
    Method::GET,
    Method::HEAD,
    Method::OPTIONS,
    Method::POST,
    Method::PUT,
    Method::DELETE,
    Method::PATCH,
    Method::TRACE,
];
const SEC_FETCH_SITE: HeaderName = HeaderName::from_static("sec-fetch-site");

fuzz_target!(|data: &[u8]| {
    let Some((&flags, rest)) = data.split_first() else {
        return;
    };
    let method = METHODS[usize::from(flags & 7)].clone();
    let absolute = flags & 8 != 0;

    let mut lines = rest.split(|&byte| byte == b'\n');
    let authority = lines.next().unwrap_or_default();
    let target = if absolute {
        let Ok(authority) = std::str::from_utf8(authority) else {
            return;
        };
        let Ok(target) = Uri::try_from(format!("http://{authority}/")) else {
            return;
        };
        target
    } else {
        Uri::from_static("/")
    };

    let mut headers = HeaderMap::new();
    for (name, line) in [header::HOST, header::ORIGIN, SEC_FETCH_SITE]
        .into_iter()
        .zip(lines)
    {
        let Ok(value) = HeaderValue::from_bytes(line) else {
            return;
        };
        headers.insert(name, value);
    }

    let mut request = Request::new(Body::empty());
    *request.method_mut() = method.clone();
    *request.uri_mut() = target.clone();
    *request.headers_mut() = headers.clone();
    let (parts, _) = request.into_parts();

    let permitted = Csrf::new().trusting_origin(TRUSTED).permits(&parts);
    let readable = |name: HeaderName| headers.get(name).and_then(|value| value.to_str().ok());
    let site = readable(SEC_FETCH_SITE);
    let origin = readable(header::ORIGIN);

    let expected = if matches!(method, Method::GET | Method::HEAD | Method::OPTIONS)
        || origin.is_some_and(|origin| origin.trim().eq_ignore_ascii_case(TRUSTED))
    {
        true
    } else if let Some(site) = site {
        matches!(site.trim(), "same-origin" | "none")
    } else if let Some(origin) = origin {
        let origin = origin.trim();
        let named = origin
            .split_once("://")
            .map_or(origin, |(_, authority)| authority);
        target
            .authority()
            .map(|authority| authority.as_str())
            .or_else(|| readable(header::HOST))
            .map(str::trim)
            .filter(|own| !own.is_empty())
            .is_some_and(|own| own.eq_ignore_ascii_case(named))
    } else {
        true
    };

    assert_eq!(permitted, expected, "{method} {target} {headers:?}");
});
