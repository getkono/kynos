//! How a [`SetCookieHeaders`] is described and written as a header group.

use kynos_openapi::model::schema::types::SchemaType;

use super::SetCookieHeaders;
use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams, UnencodableHeader},
    http::{HeaderName, HeaderValue, header::SET_COOKIE},
    schema::registry::Registry,
};

impl HeaderParams for SetCookieHeaders {
    const NAMES: &'static [&'static str] = &["set-cookie"];
    const REPEATABLE: bool = true;

    fn response_headers(
        registry: &mut Registry,
    ) -> kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>> {
        let _ = registry;

        // OpenAPI keys response headers by name, so repetition is said in prose.
        let mut headers = kynos_openapi::Map::new();
        headers.insert(
            "Set-Cookie".to_owned(),
            kynos_openapi::RefOr::Item(
                kynos_openapi::Header::new(kynos_openapi::Schema::of_type(SchemaType::String))
                    .with_description(
                        "A cookie this response sets. The field may appear more than once, which \
                         this document has no way to say: OpenAPI keys response headers by name.",
                    ),
            ),
        );
        headers
    }
}

/// Refuses the whole group where any one cookie has no
/// [`Cookie::encode`](super::Cookie::encode) spelling, rather than sending the
/// others without it.
impl EncodeHeaders for SetCookieHeaders {
    fn encode(&self) -> Result<Vec<(HeaderName, HeaderValue)>, UnencodableHeader> {
        self.cookies
            .iter()
            .map(|cookie| {
                cookie
                    .encode()
                    .map(|value| (SET_COOKIE, value))
                    .ok_or_else(|| UnencodableHeader::new(SET_COOKIE))
            })
            .collect()
    }
}
