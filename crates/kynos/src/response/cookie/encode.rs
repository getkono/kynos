//! How a [`Cookie`] is spelled as a `Set-Cookie` field value, and the grammar
//! that refuses one with no spelling.

use super::{Cookie, SameSite};
use crate::http::HeaderValue;

impl Cookie {
    /// Renders the field value.
    ///
    /// `None` where the name or the value is outside RFC 6265 section 4.1.1's
    /// grammar (the grammar defines no escaping, so nothing is escaped), where
    /// the value is wrapped in `"`, which the grammar admits but this refuses
    /// deliberately, since the reading half unwraps the quotes and the value
    /// would not read back as written, where the name and the value exceed 4096
    /// octets together, where an attribute is unrepresentable, or where a
    /// `__Host-` cookie names a `Domain` or a `Path` other than `/`. Both
    /// prefixes are matched in any case, as a user agent matches them.
    ///
    /// A handler need not call this to send the cookie: a
    /// [`SetCookieHeaders`](super::SetCookieHeaders) does, and answers 500
    /// where it returns `None`.
    #[must_use]
    pub fn encode(&self) -> Option<HeaderValue> {
        if !crate::http::is_token(self.name.as_bytes()) || !is_cookie_value(&self.value) {
            return None;
        }

        // RFC 6265bis section 5.6 step 5 measures name and value only.
        if self.name.len().saturating_add(self.value.len()) > MAX_NAME_AND_VALUE {
            return None;
        }

        // Sections 4.1.3.1 and 4.1.3.2: a user agent discards a cookie whose
        // prefix requirements are unmet, so they are enforced here.
        let host_prefixed = has_prefix(&self.name, HOST_PREFIX);
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
            rendered.push_str(&crate::http::whole_seconds(age).to_string());
        }
        // `SameSite=None`, `Partitioned` (CHIPS, draft-cutler-httpbis-
        // partitioned-cookies section 2.1) and both name prefixes are silently
        // dropped without `Secure`, so it is added for them.
        if self.secure
            || self.same_site == Some(SameSite::None)
            || self.partitioned
            || host_prefixed
            || has_prefix(&self.name, SECURE_PREFIX)
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

/// RFC 6265bis section 4.1.3.2's prefix.
const HOST_PREFIX: &str = "__Host-";

/// RFC 6265bis section 4.1.3.1's prefix.
const SECURE_PREFIX: &str = "__Secure-";

/// Whether `name` begins with `prefix` in any case.
///
/// RFC 6265bis section 5.4: "UAs MUST match cookie name prefixes
/// case-insensitively", so `__host-` is discarded on the same terms as
/// `__Host-`.
fn has_prefix(name: &str, prefix: &str) -> bool {
    name.as_bytes()
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
}

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
