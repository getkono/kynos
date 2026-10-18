//! The `#[header(...)]` grammar is closed and reads each key once: a
//! misspelled `value` was skipped, and a second `value` silently replaced the
//! first.

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

fn main() {}
