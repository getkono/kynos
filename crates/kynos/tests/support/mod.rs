//! The fixture app the integration targets share, and the one way they drive
//! it.
//!
//! Included with `#[path]` rather than depended on, because an integration
//! binary is not a library — the same reason
//! [`kynos-openapi`'s generators](../../../kynos-openapi/tests/support/mod.rs)
//! are shared that way.
//!
//! `tests/conformance.rs` deliberately does **not** use this. It is described
//! in two places as the runnable form of `examples/testing.rs`, and a reader
//! checks that correspondence by eye — which only works while the two files
//! assemble the same thing themselves.

// Each consumer takes the parts it needs; nothing here is required to be used
// by all of them.
#![allow(dead_code)]

use kynos::{
    http::{HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode, body::Body},
    prelude::*,
    response::status::NoContent,
    router::service::Service,
};
use serde::{Deserialize, Serialize};

/// The one dependency the fixture injects.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Pool(pub(crate) u32);

/// The application context.
#[derive(kynos::Provider)]
pub(crate) struct App {
    pub(crate) pool: Pool,
}

impl App {
    /// The context every fixture service is built with.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self { pool: Pool(7) }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// A user of the service.
#[derive(Debug, PartialEq, Schema, Serialize, Deserialize)]
pub(crate) struct User {
    pub(crate) id: u64,
    pub(crate) name: String,
}

/// What `/users/{id}` captures.
#[derive(Schema, kynos::PathParams)]
pub(crate) struct UserPath {
    pub(crate) id: u64,
}

/// What `/users` reads from the request target.
#[derive(Schema, kynos::QueryParams)]
pub(crate) struct UserQuery {
    pub(crate) limit: Option<u32>,
}

/// What creating a user can fail with.
#[derive(Debug, thiserror::Error, kynos::ApiError)]
#[problem(base = "https://errors.example.com/")]
pub(crate) enum StoreError {
    #[error("that name is already taken")]
    #[problem(status = 409, type = "https://errors.example.com/name-taken")]
    NameTaken,
}

/// Fetches one user.
#[kynos::get("/users/{id}")]
pub(crate) async fn get_user(Path(path): Path<UserPath>, Inject(pool): Inject<Pool>) -> Json<User> {
    Json(User {
        id: path.id,
        name: format!("user from pool {}", pool.0),
    })
}

/// Lists users, honouring an optional limit.
#[kynos::get("/users")]
pub(crate) async fn list_users(Query(query): Query<UserQuery>) -> Json<Vec<User>> {
    let names = ["Ada Lovelace", "Grace Hopper", "Barbara Liskov"];
    let wanted = query.limit.unwrap_or(u32::MAX) as usize;

    Json(
        names
            .into_iter()
            .take(wanted)
            .enumerate()
            .map(|(index, name)| User {
                id: index as u64,
                name: name.to_owned(),
            })
            .collect(),
    )
}

/// Creates a user.
#[kynos::post("/users")]
pub(crate) async fn create_user(Json(user): Json<User>) -> Result<Created<Json<User>>, StoreError> {
    if user.name == "taken" {
        return Err(StoreError::NameTaken);
    }

    Ok(Created::at(
        get_user::relative_uri(UserPath { id: user.id }),
        Json(user),
    ))
}

/// Removes one.
#[kynos::delete("/users/{id}")]
pub(crate) async fn delete_user(Path(path): Path<UserPath>) -> NoContent {
    let _ = path;
    NoContent
}

/// The four operations, unmounted, so a caller can add to them.
#[must_use]
pub(crate) fn router() -> Router<App> {
    Router::<App>::new().mount(kynos::routes![
        get_user,
        list_users,
        create_user,
        delete_user
    ])
}

/// The four operations, built.
#[must_use]
pub(crate) fn service() -> Service<App> {
    router().build(App::new()).expect("a describable router")
}

/// What one request produced.
pub(crate) struct Reply {
    pub(crate) status: StatusCode,
    pub(crate) headers: HeaderMap,
    pub(crate) body: bytes::Bytes,
}

impl Reply {
    /// The named response header as text, when it is there and printable.
    #[must_use]
    pub(crate) fn field(&self, name: &str) -> Option<String> {
        self.headers
            .get(name)
            .map(|value| value.to_str().expect("a printable field").to_owned())
    }

    /// Every value filed under `name`, in order.
    ///
    /// `Set-Cookie` is the field this exists for: it may appear more than once
    /// on one response, and `field` would report only the first.
    #[must_use]
    pub(crate) fn fields(&self, name: &str) -> Vec<String> {
        self.headers
            .get_all(name)
            .iter()
            .map(|value| value.to_str().expect("a printable field").to_owned())
            .collect()
    }

    /// The body as text.
    #[must_use]
    pub(crate) fn text(&self) -> String {
        String::from_utf8(self.body.to_vec()).expect("a printable body")
    }

    /// The body as JSON.
    #[must_use]
    pub(crate) fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|error| {
            panic!("a JSON body: {error}, from {:?}", self.text());
        })
    }
}

/// One request, built and sent.
///
/// A builder over [`Service::call`] rather than
/// [`TestClient`](kynos::test::TestClient), because `test-util` is not a
/// default feature: a target reaching for the client compiles to nothing under
/// `mise run test:baseline`, and that task is only a baseline while it stays
/// off. What is lost is the schema check, which these targets do not want —
/// `conformance.rs` is where that assertion lives.
pub(crate) struct Pending<'a, C> {
    service: &'a Service<C>,
    method: Method,
    target: String,
    headers: Vec<(HeaderName, HeaderValue)>,
    body: Option<bytes::Bytes>,
}

impl<C: 'static> Pending<'_, C> {
    /// Adds one request header.
    #[must_use]
    pub(crate) fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((
            HeaderName::from_bytes(name.as_bytes()).expect("a usable field name"),
            HeaderValue::from_str(value).expect("a usable field value"),
        ));
        self
    }

    /// Sets a JSON body and the media type that goes with it.
    #[must_use]
    pub(crate) fn json<T: Serialize>(mut self, value: &T) -> Self {
        self.body = Some(
            serde_json::to_vec(value)
                .expect("a serializable body")
                .into(),
        );
        self.header("content-type", "application/json")
    }

    /// Sets the body bytes without claiming a media type.
    #[must_use]
    pub(crate) fn body(mut self, bytes: impl Into<bytes::Bytes>) -> Self {
        self.body = Some(bytes.into());
        self
    }

    /// Drives the service and reads the whole response.
    pub(crate) async fn call(self) -> Reply {
        let mut request = Request::new(match self.body {
            Some(bytes) => Body::from_bytes(bytes),
            None => Body::empty(),
        });

        *request.method_mut() = self.method;
        *request.uri_mut() = self.target.parse().expect("a usable request target");
        for (name, value) in self.headers {
            request.headers_mut().insert(name, value);
        }

        let response = self.service.call(request).await;
        let status = response.status();
        let headers = response.headers().clone();
        let body = drain(response.into_body()).await;

        Reply {
            status,
            headers,
            body,
        }
    }
}

/// Reads a body to the bytes it carries.
async fn drain(body: Body) -> bytes::Bytes {
    use http_body_util::BodyExt;

    body.collect().await.expect("a readable body").to_bytes()
}

/// Starts a request against `service`.
pub(crate) fn send<'a, C>(service: &'a Service<C>, method: Method, target: &str) -> Pending<'a, C> {
    Pending {
        service,
        method,
        target: target.to_owned(),
        headers: Vec::new(),
        body: None,
    }
}

/// `GET target`.
pub(crate) fn get<'a, C>(service: &'a Service<C>, target: &str) -> Pending<'a, C> {
    send(service, Method::GET, target)
}

/// `POST target`.
pub(crate) fn post<'a, C>(service: &'a Service<C>, target: &str) -> Pending<'a, C> {
    send(service, Method::POST, target)
}
