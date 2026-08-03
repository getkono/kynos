//! The Info, Contact and License Objects.

use serde::{Deserialize, Serialize};

use crate::extensions::Extensions;

/// Metadata about the API.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Info {
    /// The title of the API.
    pub title: String,

    /// A short summary of the API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// A description of the API. [CommonMark] syntax may be used.
    ///
    /// [CommonMark]: https://spec.commonmark.org/
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// A URI for the Terms of Service for the API.
    #[serde(
        rename = "termsOfService",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub terms_of_service: Option<String>,

    /// Contact information for the exposed API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact: Option<Contact>,

    /// License information for the exposed API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<License>,

    /// The version of *this API document*.
    ///
    /// This is not the OpenAPI specification version — that lives on
    /// [`Document::openapi`](crate::Document::openapi) — nor is it required to
    /// be the implementation's version.
    pub version: String,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl Info {
    /// Creates the required fields of an Info Object.
    pub fn new(title: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            version: version.into(),
            ..Self::default()
        }
    }

    /// Sets the short summary.
    #[must_use]
    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// Sets the description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the contact information.
    #[must_use]
    pub fn with_contact(mut self, contact: Contact) -> Self {
        self.contact = Some(contact);
        self
    }

    /// Sets the license.
    #[must_use]
    pub fn with_license(mut self, license: License) -> Self {
        self.license = Some(license);
        self
    }
}

/// Contact information for the exposed API.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    /// The identifying name of the contact person or organization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// A URI for the contact information.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// The email address of the contact person or organization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

/// License information for the exposed API.
///
/// [`identifier`](License::identifier) and [`url`](License::url) are mutually
/// exclusive; [`crate::validate`] reports a document that sets both.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct License {
    /// The license name used for the API.
    pub name: String,

    /// An [SPDX] license expression for the API.
    ///
    /// Mutually exclusive with [`url`](License::url).
    ///
    /// [SPDX]: https://spdx.org/licenses/
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,

    /// A URI for the license used for the API.
    ///
    /// Mutually exclusive with [`identifier`](License::identifier).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl License {
    /// Creates a license identified by an SPDX expression.
    ///
    /// This is preferred over [`License::with_url`]: an SPDX identifier is
    /// machine-readable, a URL is not.
    pub fn spdx(name: impl Into<String>, identifier: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            identifier: Some(identifier.into()),
            ..Self::default()
        }
    }

    /// Creates a license identified by a URL.
    pub fn with_url(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: Some(url.into()),
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Info, License};

    #[test]
    fn info_serializes_only_the_required_fields_when_bare() {
        let json = serde_json::to_string(&Info::new("Orders", "1.0.0")).expect("serializable");
        assert_eq!(json, r#"{"title":"Orders","version":"1.0.0"}"#);
    }

    #[test]
    fn spdx_and_url_licenses_set_disjoint_fields() {
        let spdx = License::spdx("MIT", "MIT");
        assert_eq!(spdx.identifier.as_deref(), Some("MIT"));
        assert!(spdx.url.is_none());

        let url = License::with_url("MIT", "https://example.com/LICENSE");
        assert!(url.identifier.is_none());
    }
}
