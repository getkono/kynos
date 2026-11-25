//! A borrowed view of a [`TestClient`], carrying defaults of its own.
//!
//! A client's defaults are set as it is built, before a test can know a
//! credential its login request has yet to mint. A view is begun from the
//! client in use, so it can carry that credential, and a mount prefix, while
//! every request it sends still reaches the client's transport and is still
//! recorded by the client and its [`Coverage`](crate::test::Coverage).

use crate::{
    http::{HeaderMap, Method as HttpMethod, header},
    test::{IntoMethod, TestClient, TestRequest, field},
};

/// A [`TestClient`] seen under a mount prefix, a peer, and fields of its own,
/// begun by [`TestClient::view`] or [`TestClient::at`].
///
/// Every request a view begins goes through its client: the same transport,
/// recorded by the client's own assertions, such as
/// [`TestClient::assert_conformance`], and by a shared
/// [`Coverage`](crate::test::Coverage). Each setter returns a new view and
/// leaves its own unchanged, so two credentials can be interleaved in one
/// test.
///
/// A setting is taken from the nearest of the request, the view, and the
/// client: a view's field replaces the client's default of the same name,
/// and a request's own field replaces the view's.
///
/// ```
/// # async fn run(client: kynos::test::TestClient<()>, admin_token: &str, user_token: &str) {
/// let api = client.at("/api/v1");
/// let admin = api.bearer(admin_token);
/// let user = api.bearer(user_token);
///
/// // `POST /api/v1/reports`, as the user.
/// user.post("/reports").send().await.assert_status(201);
/// // `GET /api/v1/reports`, as the admin.
/// admin.get("/reports").send().await.assert_status(200);
/// // Checks both.
/// client.assert_conformance();
/// # }
/// ```
#[derive(Debug)]
pub struct ClientView<'c, C> {
    client: &'c TestClient<C>,
    /// Joined before every path, empty for none.
    prefix: String,
    /// The client's default fields, with the view's own over them.
    headers: HeaderMap,
    /// The view's peer, else the client's.
    peer: Option<std::net::SocketAddr>,
}

impl<C> Clone for ClientView<'_, C> {
    fn clone(&self) -> Self {
        Self {
            client: self.client,
            prefix: self.prefix.clone(),
            headers: self.headers.clone(),
            peer: self.peer,
        }
    }
}

impl<'c, C> ClientView<'c, C> {
    /// A view carrying the client's own defaults and nothing more.
    pub(super) fn of(client: &'c TestClient<C>) -> Self {
        Self {
            client,
            prefix: String::new(),
            headers: client.headers.clone(),
            peer: client.peer,
        }
    }

    /// Mounts the view at `prefix`, joined after any prefix it already has:
    /// `client.at("/api").at("/v1").get("/reports")` requests
    /// `/api/v1/reports`.
    ///
    /// The prefix is the string a [`Group`](crate::router::group::Group)
    /// is mounted at. A path given under a prefix is joined to it verbatim,
    /// so it begins with `/`, or with `?` for a query on the prefix itself,
    /// or is empty for the prefix alone.
    ///
    /// # Panics
    ///
    /// Panics when `prefix` does not begin with `/`, ends with `/`, or
    /// carries a query or fragment, so that no path joined to it could
    /// double a separator or land in its query.
    #[must_use = "a setter returns a new view and leaves this one unchanged"]
    #[track_caller]
    pub fn at(&self, prefix: &str) -> Self {
        assert!(
            prefix.starts_with('/') && !prefix.ends_with('/') && !prefix.contains(['?', '#']),
            "`{prefix}` is not a mount prefix: one begins with `/`, does not end with one, and \
             carries no query or fragment, as `/api/v1` does"
        );
        let mut view = self.clone();
        view.prefix.push_str(prefix);
        view
    }

    /// Presents `token` as `Authorization: Bearer <token>` (RFC 6750
    /// section 2.1) on every request, as [`TestRequest::bearer`] does on one.
    ///
    /// It replaces any `Authorization` the client or this view would send.
    ///
    /// # Panics
    ///
    /// Panics when `token` leaves the field no header value.
    #[must_use = "a setter returns a new view and leaves this one unchanged"]
    #[track_caller]
    pub fn bearer(&self, token: &str) -> Self {
        self.header("authorization", &format!("Bearer {token}"))
    }

    /// Sends the field `name: value` on every request, replacing the
    /// client's default or this view's field of the same name, unless the
    /// request sets one itself, as
    /// [`TestClient::with_default_header`] says.
    ///
    /// # Panics
    ///
    /// Panics when `name` or `value` is not one HTTP can carry, and when
    /// `name` is `Cookie` on a client given a
    /// [`with_default_cookie`](TestClient::with_default_cookie), whose cookies
    /// the field would silently replace.
    #[must_use = "a setter returns a new view and leaves this one unchanged"]
    #[track_caller]
    pub fn header(&self, name: &str, value: &str) -> Self {
        let (name, value) = field(name, value);
        assert!(
            name != header::COOKIE || self.client.cookies.is_empty(),
            "a `Cookie` header on a view would replace the client's `with_default_cookie`s; \
             send each cookie by `TestRequest::cookie` instead"
        );
        let mut view = self.clone();
        view.headers.insert(name, value);
        view
    }

    /// Sends every request from `peer`, in place of the client's
    /// [`with_peer`](TestClient::with_peer), unless the request sets its own
    /// with [`TestRequest::peer`].
    ///
    /// # Panics
    ///
    /// Panics on a client built [`connect`](TestClient::connect), whose
    /// requests leave from their own socket's address.
    #[must_use = "a setter returns a new view and leaves this one unchanged"]
    #[track_caller]
    pub fn peer(&self, peer: std::net::SocketAddr) -> Self {
        #[cfg(feature = "server")]
        self.client.refuse_peer_over_a_socket("peer");
        let mut view = self.clone();
        view.peer = Some(peer);
        view
    }

    /// Begins a `GET` request at `path`, under the view's prefix.
    ///
    /// # Panics
    ///
    /// Panics, under a prefix, when `path` is not empty and begins with
    /// neither `/` nor `?`; and so does every other method here.
    #[must_use]
    #[track_caller]
    pub fn get(&self, path: &str) -> TestRequest<'c, C> {
        self.request(HttpMethod::GET, path)
    }

    /// Begins a `POST` request at `path`, under the view's prefix.
    #[must_use]
    #[track_caller]
    pub fn post(&self, path: &str) -> TestRequest<'c, C> {
        self.request(HttpMethod::POST, path)
    }

    /// Begins a `PUT` request at `path`, under the view's prefix.
    #[must_use]
    #[track_caller]
    pub fn put(&self, path: &str) -> TestRequest<'c, C> {
        self.request(HttpMethod::PUT, path)
    }

    /// Begins a `PATCH` request at `path`, under the view's prefix.
    #[must_use]
    #[track_caller]
    pub fn patch(&self, path: &str) -> TestRequest<'c, C> {
        self.request(HttpMethod::PATCH, path)
    }

    /// Begins a `DELETE` request at `path`, under the view's prefix.
    #[must_use]
    #[track_caller]
    pub fn delete(&self, path: &str) -> TestRequest<'c, C> {
        self.request(HttpMethod::DELETE, path)
    }

    /// Begins a `HEAD` request at `path`, under the view's prefix.
    #[must_use]
    #[track_caller]
    pub fn head(&self, path: &str) -> TestRequest<'c, C> {
        self.request(HttpMethod::HEAD, path)
    }

    /// Begins an `OPTIONS` request at `path`, under the view's prefix.
    #[must_use]
    #[track_caller]
    pub fn options(&self, path: &str) -> TestRequest<'c, C> {
        self.request(HttpMethod::OPTIONS, path)
    }

    /// Begins a `TRACE` request at `path`, under the view's prefix.
    #[must_use]
    #[track_caller]
    pub fn trace(&self, path: &str) -> TestRequest<'c, C> {
        self.request(HttpMethod::TRACE, path)
    }

    /// Begins a `QUERY` request at `path`, under the view's prefix, as
    /// [`TestClient::query`] does.
    #[cfg(feature = "openapi32")]
    #[must_use]
    #[track_caller]
    pub fn query(&self, path: &str) -> TestRequest<'c, C> {
        self.request(
            HttpMethod::from_bytes(b"QUERY").expect("`QUERY` is a method token"),
            path,
        )
    }

    /// Begins a request with any method at `path`, under the view's prefix,
    /// as [`TestClient::method`] does.
    ///
    /// # Panics
    ///
    /// Panics as [`IntoMethod::into_method`] does, and as
    /// [`get`](Self::get) does for `path`.
    #[must_use]
    #[track_caller]
    pub fn method(&self, method: impl IntoMethod, path: &str) -> TestRequest<'c, C> {
        self.request(method.into_method(), path)
    }

    #[track_caller]
    fn request(&self, method: HttpMethod, path: &str) -> TestRequest<'c, C> {
        let target = self.target(path);
        self.client
            .begin(method, target, self.headers.clone(), self.peer)
    }

    /// `path` joined to the prefix.
    #[track_caller]
    fn target(&self, path: &str) -> String {
        if self.prefix.is_empty() {
            return path.to_owned();
        }
        assert!(
            path.is_empty() || path.starts_with(['/', '?']),
            "`{path}` is not a path under the view's `{}`: begin it with `/`",
            self.prefix
        );
        format!("{}{path}", self.prefix)
    }
}
