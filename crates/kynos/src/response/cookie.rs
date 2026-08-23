//! Cookies a response sets.
//!
//! The writing half. The reading half is
//! [`extract::params::cookie`](crate::extract::params::cookie) for a declared
//! parameter, and [`http::cookie`](crate::http::cookie) for the jar itself.
//!
//! # What is here and what is not
//!
//! A [`Cookie`] and a way to send it. Not a signed jar, not an encrypted one,
//! and not a session store.
//!
//! A cookie carrying a credential is a
//! [`SecurityScheme`](crate::security::SecurityScheme) rather than a parameter,
//! and signing or encrypting one is how that credential is protected — which
//! makes it authentication policy, and puts it on the wrong side of the line
//! `docs/security.md` draws. It would also arrive with a crypto stack
//! (`hmac`, `sha2`, `aes-gcm`, a source of randomness) that
//! `docs/architecture.md`'s dependency table has no row for.
//!
//! Sessions are named in that document's third invariant as the example of what
//! a layer above Kynos owns.

use std::{borrow::Cow, time::Duration};

use crate::http::HeaderValue;

/// When a cookie may accompany a cross-site request.
///
/// RFC 6265bis section 5.5.7.1. `None` requires `Secure`, which
/// [`Cookie::encode`] enforces rather than leaving to the caller: a
/// `SameSite=None` cookie without it is rejected by every current browser, and
/// silently — which is the worst way to learn.
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
    /// `Max-Age=0` rather than an `Expires` in the past. The two are equivalent
    /// to a browser, and the second would need a date to render — which means a
    /// temporal crate, every one of which is optional and off by default here.
    ///
    /// `0` is outside the ABNF, and deliberately. RFC 6265bis section 4.1.1
    /// writes `max-age-av = "Max-Age" BWS "=" BWS non-zero-digit *DIGIT`, so no
    /// spelling of "expire now" is inside it — a leading `-` is no better, since
    /// the rule admits neither. What every user agent implements is the parsing
    /// algorithm in section 5.2.2, whose step 7 reads any `delta-seconds` "less
    /// than or equal to zero" as the earliest representable date. Emitting the
    /// value the algorithm defines beats emitting one the grammar also rejects
    /// and no algorithm mentions.
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

    /// Renders the field value.
    ///
    /// `None` where the name or the value cannot appear in one: RFC 6265
    /// section 4.1.1 gives `cookie-name` the token grammar and `cookie-value`
    /// a narrower one still, and a value carrying `;` would silently become an
    /// attribute rather than part of the value.
    ///
    /// Refusing rather than escaping, because there is nothing to escape *to*:
    /// percent-encoding a cookie value is a convention a server and its own
    /// reader share, not something the grammar defines, and encoding one here
    /// would mean [`http::cookie`](crate::http::cookie) had to guess whether to
    /// decode.
    #[must_use]
    pub fn encode(&self) -> Option<HeaderValue> {
        if !is_token(&self.name) || !is_cookie_value(&self.value) {
            return None;
        }

        // RFC 6265bis section 5.6 step 5: a user agent ignores a
        // `set-cookie-string` whose name and value together exceed 4096 octets.
        // The field as a whole is not what is measured, so the attributes do
        // not count towards it.
        if self.name.len().saturating_add(self.value.len()) > MAX_NAME_AND_VALUE {
            return None;
        }

        // Sections 4.1.3.1 and 4.1.3.2. Both prefixes are enforced by the user
        // agent, which discards the whole field when its requirements are
        // unmet -- so ignoring them here means believing a cookie was set that
        // the client never stored.
        let host_prefixed = self.name.starts_with(HOST_PREFIX);
        if host_prefixed {
            // A `Domain` contradicts the prefix outright, and a `Path` naming
            // anything but `/` contradicts it too. Neither is completed:
            // dropping the first would widen the cookie past one host and
            // rewriting the second would widen it past one subtree, and
            // silently widening a cookie's scope is worse than not setting it.
            if self.domain.is_some() {
                return None;
            }
            if self.path.as_deref().is_some_and(|path| path != "/") {
                return None;
            }
        }

        let mut rendered = format!("{}={}", self.name, self.value);

        if host_prefixed {
            // Supplied rather than required, because `/` is what the prefix
            // means and it narrows nothing the caller asked for. Without it the
            // user agent derives a default path from the request URI, which is
            // not `/` and would fail the check.
            rendered.push_str("; Path=/");
        } else if let Some(path) = &self.path {
            if !is_attribute_value(path) {
                return None;
            }
            rendered.push_str("; Path=");
            rendered.push_str(path);
        }
        if let Some(domain) = &self.domain {
            if !is_domain_value(domain) {
                return None;
            }
            rendered.push_str("; Domain=");
            rendered.push_str(domain);
        }
        if let Some(age) = self.max_age {
            rendered.push_str("; Max-Age=");
            rendered.push_str(&age.as_secs().to_string());
        }
        // `SameSite=None` without `Secure` is rejected by every current browser,
        // and silently. Sending it anyway would be a cookie the service believes
        // it set and the client never stored. Both name prefixes require
        // `Secure` for the same reason and get the same treatment.
        if self.secure
            || self.same_site == Some(SameSite::None)
            || host_prefixed
            || self.name.starts_with(SECURE_PREFIX)
        {
            rendered.push_str("; Secure");
        }
        if self.http_only {
            rendered.push_str("; HttpOnly");
        }
        if let Some(same_site) = self.same_site {
            rendered.push_str("; SameSite=");
            rendered.push_str(same_site.as_str());
        }
        if self.partitioned {
            rendered.push_str("; Partitioned");
        }

        HeaderValue::from_str(&rendered).ok()
    }
}

/// The most a cookie's name and value may total, per RFC 6265bis section 5.6.
const MAX_NAME_AND_VALUE: usize = 4096;

/// RFC 6265bis section 4.1.3.2's prefix, matched case-sensitively.
const HOST_PREFIX: &str = "__Host-";

/// RFC 6265bis section 4.1.3.1's prefix, matched case-sensitively.
const SECURE_PREFIX: &str = "__Secure-";

/// RFC 6265 section 4.1.1 `domain-value`, which is a `<subdomain>` per RFC 1034
/// section 3.5 as updated by RFC 1123 section 2.1.
///
/// Narrower than [`is_attribute_value`], which is right for `Path` — a path may
/// carry a space — and wrong here: a host may not. A leading `.` is tolerated
/// because RFC 6265 section 5.2.3 strips one rather than refusing it, and
/// refusing what every user agent accepts would be stricter than the protocol.
fn is_domain_value(text: &str) -> bool {
    let text = text.strip_prefix('.').unwrap_or(text);

    !text.is_empty()
        && text.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

/// RFC 9110 section 5.6.2 `token`.
fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

/// RFC 6265 section 4.1.1 `cookie-value`: no control, whitespace, `"`, `,`, `;`
/// or `\`.
fn is_cookie_value(text: &str) -> bool {
    text.bytes()
        .all(|byte| (0x21..=0x7e).contains(&byte) && !matches!(byte, b'"' | b',' | b';' | b'\\'))
}

/// An attribute value carries anything printable except `;`, which would end it.
fn is_attribute_value(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| (0x20..=0x7e).contains(&byte) && byte != b';')
}

#[cfg(test)]
mod tests;
