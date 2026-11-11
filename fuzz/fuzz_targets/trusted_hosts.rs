//! `TrustedHosts`'s reading of the request's authority, from its target or its
//! `Host` field, and its comparison with the allowed hosts, in
//! `middleware/trusted_hosts.rs`.
//!
//! The first octet's low bit says whether the authority is the target's or
//! `Host`'s, and its second whether loopback is allowed beside the host. The
//! rest is two lines: a host to allow, and the authority.
//!
//! Allowing nothing admits nothing. The target's authority decides alone, so
//! a `Host` beside it changes nothing. Allowing the host an authority names,
//! in any ASCII case and with a trailing `.` or without, admits it on any
//! port. And under `allow_loopback` alone, an IP literal is admitted exactly
//! when the standard library calls it loopback, mapped into IPv6 or not, and
//! a name exactly when it is `localhost` or under `.localhost`.

#![no_main]

use std::net::IpAddr;

use kynos::{
    __private::fuzz::trusted_host,
    http::{HeaderMap, HeaderValue, Uri, header},
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&flags, rest)) = data.split_first() else {
        return;
    };
    let in_target = flags & 1 != 0;
    let Ok(rest) = std::str::from_utf8(rest) else {
        return;
    };
    let (allowed, named) = rest.split_once('\n').unwrap_or((rest, ""));

    let mut headers = HeaderMap::new();
    let target = if in_target {
        Some(named)
    } else {
        let Ok(value) = HeaderValue::from_str(named) else {
            return;
        };
        headers.insert(header::HOST, value);
        None
    };
    let admits =
        |allowed: &[&str], loopback: bool| trusted_host(allowed, loopback, &headers, target);

    // No authority to read, beyond not panicking on the allowed host.
    if admits(&[allowed], flags & 2 != 0).is_none() {
        return;
    }
    assert_eq!(admits(&[], false), Some(false), "{named:?}");

    if let Some(target) = target {
        let mut decoy = HeaderMap::new();
        decoy.insert(header::HOST, HeaderValue::from_static("decoy.example"));
        assert_eq!(
            trusted_host(&[allowed], false, &decoy, Some(target)),
            trusted_host(&[allowed], false, &HeaderMap::new(), Some(target)),
            "{target:?}"
        );
    }

    // The host `http`'s own grammar reads, where it reads `named` whole.
    let Ok(uri) = Uri::try_from(format!("http://{named}/")) else {
        return;
    };
    if uri.authority().map(|authority| authority.as_str()) != Some(named) {
        return;
    }
    let Some(host) = uri.host() else {
        return;
    };
    // One trailing `.` is ignored, so a host ending in two names another.
    let bare = host.strip_suffix('.').unwrap_or(host);
    if bare.is_empty() || bare.ends_with('.') {
        return;
    }

    let shouted = host.to_ascii_uppercase();
    let dotted = format!("{bare}.");
    for allowed in [host, bare, shouted.as_str(), dotted.as_str()] {
        assert_eq!(
            admits(&[allowed], false),
            Some(true),
            "{allowed:?} for {named:?}"
        );
    }

    let unbracketed = bare
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(bare);
    let loopback = match unbracketed.parse::<IpAddr>() {
        Ok(IpAddr::V4(address)) => address.is_loopback(),
        Ok(IpAddr::V6(address)) => {
            address.is_loopback()
                || address
                    .to_ipv4_mapped()
                    .is_some_and(|mapped| mapped.is_loopback())
        }
        Err(_) => {
            let name = bare.to_ascii_lowercase();
            name == "localhost" || name.ends_with(".localhost")
        }
    };
    assert_eq!(admits(&[], true), Some(loopback), "{named:?}");
});
