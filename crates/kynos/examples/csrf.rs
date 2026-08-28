//! Refusing a forged cross-site write, and the cookie the refusal protects.
//!
//! Run it with cookies on. No `server` and no `http1`: what this demonstrates
//! is which requests are refused, which is a question about header
//! combinations rather than about a socket — so it drives the built `Service`
//! directly, the way [`cors.rs`](cors.rs) does.
//!
//! ```text
//! cargo run -p kynos --example csrf --features openapi31,macros,json,cookie
//! ```
//!
//! Five things are worth noticing:
//!
//! * **There is no token, no session and no randomness.** The defence is the
//!   W3C's Fetch Metadata: a browser sets `Sec-Fetch-Site` itself and script
//!   cannot forge it, so an unsafe request that says it came from another site
//!   can be refused on that alone. A synchroniser token would need somewhere to
//!   keep the token — a session — and a crypto stack to sign it, neither of
//!   which Kynos ships. Four header comparisons need neither.
//! * **The order the rules are tried in is the order they are written.** A safe
//!   method first, then the browser's own `Sec-Fetch-Site`, then the trusted
//!   list, then an `Origin` that equals the request's own authority, and last a
//!   request carrying neither field. Each exchange below is one of those rules.
//! * **`Csrf` is mounted outside `SetCookies`, and that is the useful order.**
//!   The first `intercept` call is the outermost interceptor, so a refused
//!   request never reaches the cookie source — a 403 that also handed out a
//!   fresh session cookie would be a strange thing to send.
//! * **`SetCookies` is not a session.** It writes `Set-Cookie` and nothing else:
//!   no signing, no encryption, no server-side store. Those are application
//!   policy, and this example's session value is a fixed string standing in for
//!   whatever a real one would mint.
//! * **`SameSite` does not replace this.** It is the same defence asked of the
//!   client rather than enforced by the server: `Lax` still admits a top-level
//!   navigation, an agent that ignores the attribute ignores it silently, and
//!   neither failure is visible from here. The two are worth having together.

use std::time::Duration;

use kynos::{
    Router,
    http::{Method, Request, Response, StatusCode, header},
    middleware::{cookies::SetCookies, csrf::Csrf},
    openapi::RefOr,
    prelude::*,
    response::cookie::{Cookie, SameSite},
    router::service::Service,
};

/// The signed-in visitor.
#[derive(Schema, serde::Serialize)]
struct Profile {
    /// What to call them.
    name: String,
}

/// The cookie this service keeps a visitor on.
const SESSION: &str = "kynos_session";

/// The front end trusted to make writes from another origin.
const CONSOLE: &str = "https://admin.example.com";

/// Where this deployment answers, as a browser would address it.
const OWN_AUTHORITY: &str = "api.example.com";

/// Reads the profile. Safe, so no `Sec-Fetch-Site` can make it a forgery.
#[kynos::get("/profile")]
async fn profile() -> Json<Profile> {
    Json(Profile {
        name: "Ada Lovelace".to_owned(),
    })
}

/// Renames the visitor, which is the write worth forging.
#[kynos::post("/profile")]
async fn rename() -> NoContent {
    NoContent
}

/// Ends the session.
///
/// Unsafe, and deliberately: a forced logout is a real attack, so `DELETE` is
/// guarded exactly like the rename above.
#[kynos::delete("/session")]
async fn logout() -> NoContent {
    NoContent
}

/// What every response under this router sets, decided from the request alone.
///
/// [`CookieSource`](kynos::middleware::cookies::CookieSource) is synchronous on
/// purpose: minting a cookie is a pure function of the request, and anything
/// needing I/O to decide belongs in a handler that can also fail in a way the
/// operation declares.
fn session_cookies(request: &kynos::http::Request) -> Vec<Cookie> {
    // Logging out is the one case that writes a cookie in order to take one
    // away. `Max-Age=0` is what a removal is, and the path has to match the
    // cookie being removed: a browser keys on name, path and domain together.
    if request.method() == Method::DELETE && request.uri().path() == "/session" {
        return vec![Cookie::removal(SESSION).path("/")];
    }

    // A visitor who already has one keeps it. Re-issuing on every response
    // would cost a `Set-Cookie` field per exchange and change nothing.
    if carries_a_session(request.headers()) {
        return Vec::new();
    }

    vec![
        Cookie::new(SESSION, "opaque-identifier")
            .path("/")
            .max_age(Duration::from_secs(86_400))
            // Script cannot read it, so an injected script cannot steal it.
            .http_only()
            // Never sent in the clear.
            .secure()
            // Defence in depth beside `Csrf`, not instead of it.
            .same_site(SameSite::Lax),
    ]
}

/// Whether the request already carries this service's session cookie.
///
/// Read from the raw field rather than through
/// [`Cookies`](kynos::extract::params::cookie::Cookies), because a cookie
/// source is handed the request rather than an extraction — and because a
/// session cookie is a credential, which [`parameters.rs`](parameters.rs) notes
/// is a security scheme rather than a parameter.
fn carries_a_session(headers: &kynos::http::HeaderMap) -> bool {
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|jar| {
            jar.split(';')
                .any(|pair| pair.trim().starts_with(&format!("{SESSION}=")))
        })
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new()
        .mount(kynos::routes![profile, rename, logout])
        // Outermost, so nothing below runs for a request it refuses.
        //
        // The trusted list is compared byte for byte after lowercasing and has
        // no wildcard: an allow-list that admits a subdomain admits whoever
        // takes that subdomain over.
        .intercept(Csrf::new().trusting_origin(CONSOLE))
        .intercept(SetCookies::new(
            |request: &kynos::http::Request, (): &()| session_cookies(request),
        ));

    what_the_description_says(&router.openapi()?);

    let service = router.build(())?;

    a_safe_method_is_never_refused(&service).await;
    a_same_origin_write_is_served(&service).await;
    a_cross_site_write_is_refused(&service).await;
    a_trusted_origin_is_admitted(&service).await;
    an_origin_equal_to_the_host_is_admitted(&service).await;
    a_client_that_is_not_a_browser_is_not_subject(&service).await;
    logging_out_removes_the_cookie(&service).await;

    Ok(())
}

/// Both interceptors reached the document, and neither was written down twice.
///
/// `Csrf` contributes the 403 because `Short` *is* the declaration, and
/// `SetCookies` contributes the `Set-Cookie` entry because `Adds` is. No
/// handler below either one lists a status or a field of its own.
fn what_the_description_says(document: &kynos::openapi::Document) {
    let write = document.paths.items["/profile"]
        .post
        .as_ref()
        .expect("the rename is declared");

    println!(
        "POST /profile declares {:?}",
        write.responses.responses.keys().collect::<Vec<_>>()
    );
    assert!(write.responses.responses.contains_key("403"));

    let read = document.paths.items["/profile"]
        .get
        .as_ref()
        .expect("the profile is declared");
    let RefOr::Item(ok) = &read.responses.responses["200"] else {
        panic!("the success is inline rather than a reference")
    };

    // One entry, and the prose beside it says the field may repeat: OpenAPI
    // keys response headers by name and has no vocabulary for a field that
    // appears twice, which RFC 6265 requires of `Set-Cookie`.
    println!(
        "GET /profile declares {:?}",
        ok.headers.keys().collect::<Vec<_>>()
    );
    assert!(ok.headers.contains_key("Set-Cookie"));
}

/// Rule 1: `GET`, `HEAD` and `OPTIONS` are read-only, so forging one achieves
/// nothing a link could not.
async fn a_safe_method_is_never_refused(service: &Service<()>) {
    let response = send(
        service,
        Method::GET,
        "/profile",
        &[("sec-fetch-site", "cross-site")],
    )
    .await;

    println!("\nGET from another site -> {}", response.status());
    assert_eq!(response.status(), StatusCode::OK);

    // The visitor had no session, so this is where they get one.
    show(&response, &header::SET_COOKIE);
}

/// Rule 2, the affirmative half: the browser states the request came from here.
async fn a_same_origin_write_is_served(service: &Service<()>) {
    let response = send(
        service,
        Method::POST,
        "/profile",
        &[("sec-fetch-site", "same-origin")],
    )
    .await;

    println!("\nPOST, same-origin -> {}", response.status());
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

/// Rule 2, the refusal: the same field, stating the opposite.
///
/// This is the whole defence in one exchange. `Sec-Fetch-Site` is set by the
/// browser and forbidden to script, so `cross-site` is a fact rather than a
/// claim the attacker controls.
async fn a_cross_site_write_is_refused(service: &Service<()>) {
    let response = send(
        service,
        Method::POST,
        "/profile",
        &[("sec-fetch-site", "cross-site")],
    )
    .await;

    println!("\nPOST, cross-site -> {}", response.status());
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // No cookie: `Csrf` short-circuited outside `SetCookies`, so the source was
    // never asked.
    assert!(response.headers().get(header::SET_COOKIE).is_none());
    println!("  set-cookie: <absent>");
}

/// Rule 3: a front end served from somewhere else, named explicitly.
///
/// No `Sec-Fetch-Site` here, which is what an older browser sends — it still
/// sends `Origin` on an unsafe request.
async fn a_trusted_origin_is_admitted(service: &Service<()>) {
    let response = send(service, Method::POST, "/profile", &[("origin", CONSOLE)]).await;

    println!("\nPOST from the trusted console -> {}", response.status());
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

/// Rule 4: an `Origin` whose authority is the request's own.
///
/// `Host` is read as well as the target's authority, because only HTTP/1.1
/// carries it in a field: RFC 9113 replaces `Host` with `:authority`, which
/// `http` puts on the URI instead.
async fn an_origin_equal_to_the_host_is_admitted(service: &Service<()>) {
    let response = send(
        service,
        Method::POST,
        "/profile",
        &[
            ("origin", &format!("https://{OWN_AUTHORITY}")),
            ("host", OWN_AUTHORITY),
        ],
    )
    .await;

    println!("\nPOST from our own origin -> {}", response.status());
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

/// Rule 5: neither field, which no browser does.
///
/// `curl`, a mobile client and a server carry no ambient credentials on another
/// site's behalf, so none of them is subject to CSRF and refusing them would
/// buy nothing. Worth being explicit about, because it is the rule that looks
/// like a hole and is not one.
async fn a_client_that_is_not_a_browser_is_not_subject(service: &Service<()>) {
    let response = send(service, Method::POST, "/profile", &[]).await;

    println!(
        "\nPOST from something that is not a browser -> {}",
        response.status()
    );
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

/// The other thing a cookie writer is for.
async fn logging_out_removes_the_cookie(service: &Service<()>) {
    let response = send(
        service,
        Method::DELETE,
        "/session",
        &[
            ("sec-fetch-site", "same-origin"),
            ("cookie", &format!("{SESSION}=opaque-identifier")),
        ],
    )
    .await;

    println!("\nDELETE /session -> {}", response.status());
    show(&response, &header::SET_COOKIE);
    assert!(
        response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("Max-Age=0"))
    );
}

/// Drives the built service directly, which is all an exchange is here.
async fn send(
    service: &Service<()>,
    method: Method,
    path: &str,
    fields: &[(&str, &str)],
) -> Response {
    let mut request = Request::new(kynos::http::body::Body::empty());
    *request.method_mut() = method;
    *request.uri_mut() = path.parse().expect("a usable path");

    for (name, value) in fields {
        request.headers_mut().insert(
            header::HeaderName::from_bytes(name.as_bytes()).expect("a usable field name"),
            kynos::http::HeaderValue::from_str(value).expect("a usable field value"),
        );
    }

    service.call(request).await
}

/// Prints one response header, when it is there.
fn show(response: &Response, name: &header::HeaderName) {
    if let Some(value) = response.headers().get(name) {
        println!("  {name}: {}", value.to_str().unwrap_or("<unprintable>"));
    }
}
