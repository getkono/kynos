//! Cookies a response sets.
//!
//! The writing half. The reading half is
//! [`extract::params::cookie`](crate::extract::params::cookie) for a declared
//! parameter, and [`http::cookie`](crate::http::cookie) for the jar itself.
//!
//! # What is here and what is not
//!
//! A [`Cookie`], and [`SetCookieHeaders`], the response header group that sends
//! cookies. A handler returns that group in a
//! [`WithHeaders`](crate::response::headers::WithHeaders) for a cookie it mints
//! per request; [`SetCookies`](crate::middleware::cookies::SetCookies) adds it
//! to every response an operation produces. Not a signed jar, not an encrypted one,
//! and not a session store: a cookie carrying a credential is a
//! [`SecurityScheme`](crate::security::SecurityScheme), and sessions belong to
//! a layer above Kynos.

use std::{borrow::Cow, time::Duration};

mod encode;
mod header;

/// When a cookie may accompany a cross-site request.
///
/// RFC 6265bis section 5.5.7.1. `None` requires `Secure`, which
/// [`Cookie::encode`] adds, since browsers silently reject the cookie without it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum SameSite {
    /// Never sent cross-site.
    Strict,
    /// Sent on a top-level navigation. The browser default.
    #[default]
    Lax,
    /// Sent on every cross-site request. Implies `Secure`.
    None,
}

impl SameSite {
    /// The attribute value, as it is spelled on the wire.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "Strict",
            Self::Lax => "Lax",
            Self::None => "None",
        }
    }
}

/// One `Set-Cookie` value.
///
/// A response sends it as a [`SetCookieHeaders`], which a cookie converts into
/// with `into()`.
///
/// ```
/// use kynos::response::cookie::{Cookie, SameSite};
///
/// let session = Cookie::new("locale", "en-GB")
///     .path("/")
///     .max_age(std::time::Duration::from_secs(86_400))
///     .http_only()
///     .same_site(SameSite::Strict);
///
/// assert_eq!(
///     session.encode().expect("a representable cookie").to_str().unwrap(),
///     "locale=en-GB; Path=/; Max-Age=86400; HttpOnly; SameSite=Strict",
/// );
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cookie {
    name: Cow<'static, str>,
    value: Cow<'static, str>,
    path: Option<Cow<'static, str>>,
    domain: Option<Cow<'static, str>>,
    max_age: Option<Duration>,
    secure: bool,
    http_only: bool,
    same_site: Option<SameSite>,
    partitioned: bool,
}

impl Cookie {
    /// A cookie called `name` carrying `value`.
    #[must_use]
    pub fn new(name: impl Into<Cow<'static, str>>, value: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            path: None,
            domain: None,
            max_age: None,
            secure: false,
            http_only: false,
            same_site: None,
            partitioned: false,
        }
    }

    /// A cookie that deletes the one of the same name.
    ///
    /// Sends `Max-Age=0`. RFC 6265bis section 4.1.1's grammar has no spelling
    /// of "expire now", but every user agent follows section 5.2.2, which reads
    /// a non-positive `Max-Age` as the earliest representable date.
    ///
    /// The `path` and `domain` have to match the cookie being deleted, because
    /// a browser keys on all three.
    #[must_use]
    pub fn removal(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            max_age: Some(Duration::ZERO),
            ..Self::new(name, "")
        }
    }

    /// The path the cookie is scoped to.
    #[must_use]
    pub fn path(mut self, path: impl Into<Cow<'static, str>>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// The domain the cookie is scoped to.
    #[must_use]
    pub fn domain(mut self, domain: impl Into<Cow<'static, str>>) -> Self {
        self.domain = Some(domain.into());
        self
    }

    /// How long the cookie lives.
    ///
    /// `Max-Age` counts whole seconds, so a fractional `age` is rounded up: a
    /// cookie never lives shorter than asked, and only `Duration::ZERO` sends
    /// `Max-Age=0`, which a user agent reads as removal. Use
    /// [`Cookie::removal`] to say that on purpose.
    #[must_use]
    pub fn max_age(mut self, age: Duration) -> Self {
        self.max_age = Some(age);
        self
    }

    /// Sends the cookie only over a secure connection.
    #[must_use]
    pub fn secure(mut self) -> Self {
        self.secure = true;
        self
    }

    /// Hides the cookie from scripts.
    #[must_use]
    pub fn http_only(mut self) -> Self {
        self.http_only = true;
        self
    }

    /// When the cookie accompanies a cross-site request.
    #[must_use]
    pub fn same_site(mut self, same_site: SameSite) -> Self {
        self.same_site = Some(same_site);
        self
    }

    /// Partitions the cookie by top-level site.
    ///
    /// Implies `Secure`, which is sent whether or not it was asked for: a user
    /// agent ignores a partitioned cookie without it.
    #[must_use]
    pub fn partitioned(mut self) -> Self {
        self.partitioned = true;
        self
    }

    /// The name this cookie is filed under.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// The `Set-Cookie` fields one response sends.
///
/// A handler sets a cookie minted for this request, such as a session
/// credential, by returning it in
/// [`WithHeaders`](crate::response::headers::WithHeaders); the description then
/// declares `Set-Cookie` on that operation. A cookie every response of an
/// operation sets belongs in
/// [`SetCookies`](crate::middleware::cookies::SetCookies) instead, which adds
/// this same group.
///
/// ```
/// use kynos::response::{
///     cookie::{Cookie, SameSite, SetCookieHeaders},
///     headers::WithHeaders,
///     status::NoContent,
/// };
///
/// async fn login() -> WithHeaders<NoContent, SetCookieHeaders> {
///     let token = "minted-per-request";
///     let session = Cookie::new("sid", token)
///         .path("/")
///         .http_only()
///         .same_site(SameSite::Strict);
///
///     WithHeaders::new(NoContent, session.into())
/// }
/// ```
///
/// One cookie converts with `into()`, as does a `Vec` of them, and an iterator
/// of cookies collects into one.
///
/// `REPEATABLE` is `true`: RFC 6265 forbids comma-joining two `Set-Cookie`
/// values, so each cookie is sent as its own field. Where any cookie has no
/// [`Cookie::encode`] spelling, the response is answered with a 500 in its
/// place and none of the cookies is set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SetCookieHeaders {
    /// The cookies this response sets.
    pub cookies: Vec<Cookie>,
}

/// The one cookie a response sets.
impl From<Cookie> for SetCookieHeaders {
    fn from(cookie: Cookie) -> Self {
        Self {
            cookies: vec![cookie],
        }
    }
}

/// Each cookie, sent as its own field in this order.
impl From<Vec<Cookie>> for SetCookieHeaders {
    fn from(cookies: Vec<Cookie>) -> Self {
        Self { cookies }
    }
}

/// Each cookie, sent as its own field in this order.
impl FromIterator<Cookie> for SetCookieHeaders {
    fn from_iter<I: IntoIterator<Item = Cookie>>(cookies: I) -> Self {
        Self {
            cookies: cookies.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests;
