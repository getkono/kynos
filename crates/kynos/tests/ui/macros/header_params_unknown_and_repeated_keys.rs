//! The `#[header(...)]` grammar is closed and reads each key once: a
//! misspelled `value` was skipped, and a second `value` silently replaced the
//! first. The struct's own `#[header(...)]` takes `described` alone, once, as
//! a `bool` literal.

#[derive(kynos::HeaderParams)]
struct Tracing {
    #[header(renmae = "X-Request-Id")]
    request_id: String,
}

#[derive(kynos::HeaderParams)]
struct Hardened {
    #[header(rename = "X-Content-Type-Options", value = "nosniff")]
    #[header(value = "sniff")]
    nosniff: (),
}

#[derive(kynos::HeaderParams)]
#[header(rename = "Origin")]
struct Provenance {
    #[header(rename = "Origin")]
    origin: Option<String>,
}

#[derive(kynos::HeaderParams)]
#[header(described = false)]
#[header(described = true)]
struct Referral {
    #[header(rename = "Referer")]
    referer: Option<String>,
}

#[derive(kynos::HeaderParams)]
#[header(described = "false")]
struct Agent {
    #[header(rename = "User-Agent")]
    user_agent: Option<String>,
}

fn main() {}
