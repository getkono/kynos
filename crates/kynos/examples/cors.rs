//! Cross-origin resource sharing, end to end.
//!
//! What a browser actually does with a cross-origin API is two exchanges, not
//! one: a preflight `OPTIONS` asking whether the real request is allowed, then
//! the real request. Kynos answers the first without an operation being
//! declared for it, and marks the second.
//!
//! Three things worth watching for:
//!
//! * **The preflight has no handler.** Nothing below declares `OPTIONS`. The
//!   router registers the answer while the service is built, after the
//!   description has been assembled — so it contributes no `paths` key, appears
//!   in no `Allow` header, and runs no interceptor.
//! * **The origins come from the environment.** That is why the list-taking
//!   builders accept anything iterable of anything string-like: an allow-list is
//!   deployment configuration, not a compile-time constant.
//! * **A configuration the protocol forbids is refused, not served.** The last
//!   section proves it.
//!
//! Run with:
//!
//! ```text
//! cargo run -p kynos --example cors --features openapi31,macros,json
//! ```

use kynos::{
    Router,
    http::{Method, Request, Response, StatusCode, header},
    middleware::cors::Cors,
    response::status::NoContent,
    router::service::Service,
};

/// A widget.
#[derive(kynos::Schema, serde::Serialize)]
struct Widget {
    /// What it is called.
    name: String,
}

/// Lists the widgets.
#[kynos::get("/widgets")]
async fn list_widgets() -> kynos::extract::body::json::Json<Vec<Widget>> {
    kynos::extract::body::json::Json(vec![Widget {
        name: "sprocket".to_owned(),
    }])
}

/// Removes one.
///
/// `DELETE` is not a "simple" method, so a browser preflights it — which is the
/// exchange this example exists to show.
#[kynos::delete("/widgets")]
async fn delete_widget() -> NoContent {
    NoContent
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    // The shape a real deployment takes: read at startup, owned rather than
    // borrowed, and applied without the type of the builder changing.
    let permitted: Vec<String> = std::env::var("CORS_ORIGINS")
        .unwrap_or_else(|_| "https://app.example.com".to_owned())
        .split(',')
        .map(|origin| origin.trim().to_owned())
        .collect();

    let service = Router::<()>::new()
        .mount(kynos::routes![list_widgets, delete_widget])
        .intercept(
            Cors::new()
                .allow_origins(permitted)
                .allow_headers(["x-trace-id"])
                .expose_headers(["x-request-id"])
                .max_age(std::time::Duration::from_secs(600)),
        )
        .build(())?;

    preflight(&service).await;
    real_request(&service).await;
    refused_origin(&service).await;
    a_configuration_that_cannot_be_honoured();

    Ok(())
}

/// The exchange no operation declares.
async fn preflight(service: &Service<()>) {
    let response = send(
        service,
        Method::OPTIONS,
        &[
            ("origin", "https://app.example.com"),
            ("access-control-request-method", "DELETE"),
            ("access-control-request-headers", "x-trace-id"),
        ],
    )
    .await;

    println!("preflight -> {}", response.status());
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // Derived from the operations declared on the path, so what the preflight
    // advertises and what the description promises cannot disagree.
    show(&response, header::ACCESS_CONTROL_ALLOW_METHODS);
    show(&response, header::ACCESS_CONTROL_ALLOW_HEADERS);
    show(&response, header::ACCESS_CONTROL_MAX_AGE);

    // All three fields the answer read, so a cache cannot reuse a `DELETE`
    // preflight's answer for a `PATCH`.
    show(&response, header::VARY);
}

/// The request the preflight was about.
async fn real_request(service: &Service<()>) {
    let response = send(
        service,
        Method::GET,
        &[("origin", "https://app.example.com")],
    )
    .await;

    println!("\nreal request -> {}", response.status());
    show(&response, header::ACCESS_CONTROL_ALLOW_ORIGIN);
    show(&response, header::ACCESS_CONTROL_EXPOSE_HEADERS);

    // `Vary: Origin` on the real response too. Without it a shared cache would
    // hand one origin's `Access-Control-Allow-Origin` to another, which is the
    // whole of the CORS check defeated.
    show(&response, header::VARY);
}

/// An origin the allow-list does not name.
async fn refused_origin(service: &Service<()>) {
    let response = send(
        service,
        Method::GET,
        &[("origin", "https://evil.example.com")],
    )
    .await;

    println!("\nrefused origin -> {}", response.status());

    // The protocol reads an absent header as a refusal, so the answer is the
    // ordinary response with no CORS header on it — not an error status. The
    // browser is what refuses to hand the body to the page.
    assert!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );
    println!("  access-control-allow-origin: <absent>");
}

/// The CORS protocol forbids `Access-Control-Allow-Origin: *` on a credentialed
/// response, so there is no answer satisfying both.
///
/// Refused while the router is built rather than served: the fallback would be
/// to echo the request's own origin, which turns "any origin" plus "credentials"
/// into reflect-any-origin-with-credentials — the most permissive configuration
/// the protocol has, reached by asking for something else.
fn a_configuration_that_cannot_be_honoured() {
    let refused = Router::<()>::new()
        .mount(kynos::routes![list_widgets])
        .intercept(Cors::new().allow_any_origin().allow_credentials())
        .build(());

    println!(
        "\nany origin + credentials -> {}",
        match &refused {
            Ok(_) => "accepted".to_owned(),
            Err(error) => error.to_string(),
        }
    );

    assert!(refused.is_err());
}

/// Drives the built service directly, which is all a browser exchange is here.
async fn send(service: &Service<()>, method: Method, fields: &[(&str, &str)]) -> Response {
    let mut request = Request::new(kynos::http::body::Body::empty());
    *request.method_mut() = method;
    *request.uri_mut() = "/widgets".parse().expect("a usable path");

    for (name, value) in fields {
        request.headers_mut().insert(
            header::HeaderName::from_bytes(name.as_bytes()).expect("a usable field name"),
            kynos::http::HeaderValue::from_str(value).expect("a usable field value"),
        );
    }

    service.call(request).await
}

/// Prints one response header, when it is there.
fn show(response: &Response, name: header::HeaderName) {
    if let Some(value) = response.headers().get(&name) {
        println!("  {name}: {}", value.to_str().unwrap_or("<unprintable>"));
    }
}
