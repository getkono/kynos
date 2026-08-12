//! The Info, Contact and License Objects.

use serde::{Deserialize, Serialize};

use crate::model::extensions::Extensions;

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
/// Every version of the specification makes `identifier` and `url` mutually
/// exclusive, and makes both optional beside a required `name`. This type holds
/// at most one of the two, so the three states the specification allows are the
/// three a program can build — and a document setting both cannot be
/// constructed, nor parsed, nor emitted.
///
/// Reach for [`spdx`](License::spdx) in preference to
/// [`with_url`](License::with_url): an SPDX expression is machine-readable and a
/// URL is not. Both are permitted, which is why both are here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawLicense", into = "RawLicense")]
pub struct License {
    name: String,
    link: Option<LicenseLink>,

    /// Specification extensions.
    pub extensions: Extensions,
}

/// How a license points at its terms, when it does.
///
/// An enum rather than two `Option` fields, for the reason
/// [`SecurityScheme`](crate::model::security::SecurityScheme) is one: an
/// unusable combination that cannot be spelled needs no rule to reject it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum LicenseLink {
    /// An [SPDX] license expression.
    ///
    /// [SPDX]: https://spdx.org/licenses/
    Spdx(String),

    /// A URI for the license text.
    Url(String),
}

impl License {
    /// Creates a license identified by name alone.
    ///
    /// Valid, and the weakest of the three: a consumer gets something to show a
    /// human and nothing to resolve.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            link: None,
            extensions: Extensions::default(),
        }
    }

    /// Creates a license identified by an SPDX expression.
    ///
    /// This is preferred over [`License::with_url`]: an SPDX identifier is
    /// machine-readable, a URL is not.
    pub fn spdx(name: impl Into<String>, identifier: impl Into<String>) -> Self {
        Self {
            link: Some(LicenseLink::Spdx(identifier.into())),
            ..Self::named(name)
        }
    }

    /// Creates a license identified by a URL.
    pub fn with_url(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            link: Some(LicenseLink::Url(url.into())),
            ..Self::named(name)
        }
    }

    /// The license name used for the API.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The [SPDX] license expression, when this license carries one.
    ///
    /// [SPDX]: https://spdx.org/licenses/
    #[must_use]
    pub fn identifier(&self) -> Option<&str> {
        match &self.link {
            Some(LicenseLink::Spdx(identifier)) => Some(identifier),
            _ => None,
        }
    }

    /// The URI for the license, when this license carries one.
    #[must_use]
    pub fn url(&self) -> Option<&str> {
        match &self.link {
            Some(LicenseLink::Url(url)) => Some(url),
            _ => None,
        }
    }
}

/// The wire shape: two flat optional fields, as the specification writes them.
///
/// The exclusion is enforced crossing this boundary rather than inside
/// [`License`], so the invariant holds for a parsed document as well as a built
/// one.
#[derive(Serialize, Deserialize)]
struct RawLicense {
    name: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    identifier: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    url: Option<String>,

    #[serde(flatten)]
    extensions: Extensions,
}

/// A License Object that set both `identifier` and `url`.
#[derive(Debug)]
struct LicenseConflict;

impl std::fmt::Display for LicenseConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("`identifier` and `url` are mutually exclusive on a License Object")
    }
}

impl TryFrom<RawLicense> for License {
    type Error = LicenseConflict;

    fn try_from(raw: RawLicense) -> Result<Self, Self::Error> {
        let link = match (raw.identifier, raw.url) {
            (Some(_), Some(_)) => return Err(LicenseConflict),
            (Some(identifier), None) => Some(LicenseLink::Spdx(identifier)),
            (None, Some(url)) => Some(LicenseLink::Url(url)),
            (None, None) => None,
        };

        Ok(Self {
            name: raw.name,
            link,
            extensions: raw.extensions,
        })
    }
}

impl From<License> for RawLicense {
    fn from(license: License) -> Self {
        let (identifier, url) = match license.link {
            Some(LicenseLink::Spdx(identifier)) => (Some(identifier), None),
            Some(LicenseLink::Url(url)) => (None, Some(url)),
            None => (None, None),
        };

        Self {
            name: license.name,
            identifier,
            url,
            extensions: license.extensions,
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
        assert_eq!(spdx.identifier(), Some("MIT"));
        assert!(spdx.url().is_none());

        let url = License::with_url("MIT", "https://example.com/LICENSE");
        assert!(url.identifier().is_none());
        assert_eq!(url.url(), Some("https://example.com/LICENSE"));

        let bare = License::named("Proprietary");
        assert!(bare.identifier().is_none());
        assert!(bare.url().is_none());
    }

    #[test]
    fn each_license_shape_round_trips_through_its_wire_form() {
        for license in [
            License::named("Proprietary"),
            License::spdx("MIT", "MIT"),
            License::with_url("MIT", "https://example.com/LICENSE"),
        ] {
            let json = serde_json::to_string(&license).expect("serializable");
            let parsed: License = serde_json::from_str(&json).expect("deserializable");
            assert_eq!(parsed, license);
        }
    }

    #[test]
    fn a_license_serializes_only_the_link_it_carries() {
        let json = serde_json::to_string(&License::spdx("MIT", "MIT")).expect("serializable");
        assert_eq!(json, r#"{"name":"MIT","identifier":"MIT"}"#);
    }

    #[test]
    fn a_license_setting_both_links_does_not_deserialize() {
        let error = serde_json::from_str::<License>(
            r#"{"name":"MIT","identifier":"MIT","url":"https://example.com/LICENSE"}"#,
        )
        .expect_err("`identifier` and `url` are mutually exclusive");

        assert!(
            error.to_string().contains("mutually exclusive"),
            "the rejection should say why: {error}"
        );
    }
}
