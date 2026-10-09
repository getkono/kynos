//! Cookies a response sets.
//!
//! The writing half. The reading half is
//! [`extract::params::cookie`](crate::extract::params::cookie) for a declared
//! parameter, and [`http::cookie`](crate::http::cookie) for the jar itself.
//!
//! # What is here and what is not
//!
//! A [`Cookie`] and a way to send it. Not a signed jar, not an encrypted one,
//! and not a session store: a cookie carrying a credential is a
//! [`SecurityScheme`](crate::security::SecurityScheme), and sessions belong to
//! a layer above Kynos.

use std::{borrow::Cow, time::Duration};

use crate::http::HeaderValue;

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
    /// `None` where the name or the value is outside RFC 6265 section 4.1.1's
    /// grammar (the grammar defines no escaping, so nothing is escaped), where
    /// they exceed 4096 octets together, where an attribute is unrepresentable,
    /// or where a `__Host-` cookie names a `Domain` or a `Path` other than `/`.
    #[must_use]
    pub fn encode(&self) -> Option<HeaderValue> {
        if !crate::http::is_token(&self.name) || !is_cookie_value(&self.value) {
            return None;
        }

        // RFC 6265bis section 5.6 step 5 measures name and value only.
        if self.name.len().saturating_add(self.value.len()) > MAX_NAME_AND_VALUE {
            return None;
        }

        // Sections 4.1.3.1 and 4.1.3.2: a user agent discards a cookie whose
        // prefix requirements are unmet, so they are enforced here.
        let host_prefixed = self.name.starts_with(HOST_PREFIX);
        if host_prefixed {
            // Refused rather than corrected: dropping either would silently
            // widen the cookie's scope.
            if self.domain.is_some() {
                return None;
            }
            if self.path.as_deref().is_some_and(|path| path != "/") {
                return None;
            }
        }

        let mut rendered = format!("{}={}", self.name, self.value);

        if host_prefixed {
            // Supplied, since the default path derived from the request URI
            // would fail the prefix check.
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
        // `SameSite=None` and both name prefixes are silently dropped without
        // `Secure`, so it is added for them.
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
/// A leading `.` is tolerated because RFC 6265 section 5.2.3 strips one.
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
