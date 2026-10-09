//! One described operation per file.

use kynos_openapi::{Method, PathTemplate};

use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams},
    http::{HeaderValue, Request, Response, StatusCode, etag, header},
    response::range::spec,
    router::{
        assets::{Asset, range},
        endpoint::{Endpoint, operation_id},
        operation::OperationCx,
    },
};

/// The fields an asset response carries, declared so the conflict check sees
/// them.
#[derive(Clone, Copy, Debug)]
struct AssetHeaders {
    etag: &'static str,
    cache_control: Option<&'static str>,
    /// The coding the selected representation is in, if it is not identity.
    coding: Option<&'static str>,
    /// Whether this file has stored codings, and so sends `Vary` on every
    /// response; a file without would partition cache keys for nothing.
    negotiable: bool,
}

impl AssetHeaders {
    /// The same group, as a 304 is allowed to carry it: without
    /// `Content-Encoding`, which RFC 9110 section 15.4.5 does not list.
    fn not_modified(mut self) -> Self {
        self.coding = None;
        self
    }
}

impl HeaderParams for AssetHeaders {
    const NAMES: &'static [&'static str] = &["etag", "cache-control", "content-encoding", "vary"];
    // No `VARIES`: it is per type, and only files with stored codings vary.
}

impl EncodeHeaders for AssetHeaders {
    fn encode(&self) -> Vec<(crate::http::HeaderName, HeaderValue)> {
        let mut fields = Vec::with_capacity(4);

        if let Ok(value) = HeaderValue::from_str(self.etag) {
            fields.push((header::ETAG, value));
        }
        if let Some(cache_control) = self.cache_control {
            if let Ok(value) = HeaderValue::from_str(cache_control) {
                fields.push((header::CACHE_CONTROL, value));
            }
        }
        if let Some(coding) = self.coding {
            if let Ok(value) = HeaderValue::from_str(coding) {
                fields.push((header::CONTENT_ENCODING, value));
            }
        }
        if self.negotiable {
            fields.push((header::VARY, HeaderValue::from_static("accept-encoding")));
        }

        fields
    }
}

/// One file, served at one path.
///
/// The path is fixed before the router is built and never joined with request
/// input, so there is no traversal to defend against.
#[derive(Clone, Debug)]
pub struct AssetEndpoint {
    asset: Asset,
    template: PathTemplate,
    operation_id: String,
    cache_control: Option<&'static str>,
}

impl AssetEndpoint {
    /// Serves `asset` at `path`, relative to wherever the set is mounted.
    ///
    /// # Panics
    ///
    /// If `path` is not a legal path template, which only a hand-built `Asset`
    /// can carry.
    #[must_use]
    pub(super) fn new(
        asset: Asset,
        path: &str,
        cache_control: Option<&'static str>,
        prefix: &str,
    ) -> Self {
        let relative = format!("/{}", path.trim_start_matches('/'));
        let template = PathTemplate::parse(&relative)
            .unwrap_or_else(|error| panic!("`{relative}` is not a servable asset path: {error}"));

        Self {
            asset,
            template,
            operation_id: operation_id(prefix, path),
            cache_control,
        }
    }

    /// The group both a success and a 304 carry, for the representation chosen.
    fn headers(&self, chosen: &Representation) -> AssetHeaders {
        AssetHeaders {
            etag: chosen.etag,
            cache_control: self.cache_control,
            coding: chosen.coding,
            negotiable: !self.asset.encodings().is_empty(),
        }
    }

    /// The representation this request gets; chosen first, since every
    /// condition and range is evaluated against it.
    fn choose(&self, headers: &crate::http::HeaderMap) -> Representation {
        let identity = Representation {
            bytes: self.asset.bytes(),
            etag: self.asset.etag(),
            coding: None,
        };

        if self.asset.encodings().is_empty() {
            return identity;
        }

        let Some(accept) = headers
            .get(header::ACCEPT_ENCODING)
            .and_then(|value| value.to_str().ok())
        else {
            // No field (RFC 9110 section 12.5.3): identity, which every client reads.
            return identity;
        };

        let available: Vec<&str> = self
            .asset
            .encodings()
            .iter()
            .map(super::Encoded::coding)
            .collect();

        let Some(coding) = crate::http::coding::preferred(accept, &available) else {
            return identity;
        };

        self.asset
            .encodings()
            .iter()
            .find(|encoded| encoded.coding() == coding)
            .map_or(identity, |encoded| Representation {
                bytes: encoded.bytes(),
                etag: encoded.etag(),
                coding: Some(encoded.coding()),
            })
    }

    /// The response fields each status carries, declared only where they can
    /// be sent.
    fn declare_response_headers(&self, operation: &mut OperationCx<'_>) {
        for (status, name, description) in [
            (200, "ETag", "The entity tag of this representation"),
            (206, "ETag", "The entity tag of this representation"),
            (304, "ETag", "The entity tag of this representation"),
            (
                200,
                "Cache-Control",
                "How long this representation may be reused",
            ),
            (
                206,
                "Cache-Control",
                "How long this representation may be reused",
            ),
            (
                200,
                "Content-Encoding",
                "The coding the stored representation is in, absent for identity",
            ),
            (
                206,
                "Content-Encoding",
                "The coding the stored representation is in, absent for identity",
            ),
            (
                200,
                "Vary",
                "Names Accept-Encoding, since this file has more than one stored coding",
            ),
            (
                206,
                "Vary",
                "Names Accept-Encoding, since this file has more than one stored coding",
            ),
            (
                304,
                "Vary",
                "Names Accept-Encoding, since this file has more than one stored coding",
            ),
        ] {
            if name == "Cache-Control" && self.cache_control.is_none() {
                continue;
            }
            // Both are sent only where a coding was stored; at 304 only `Vary`
            // is (RFC 9110 section 15.4.5).
            if (name == "Content-Encoding" || name == "Vary") && self.asset.encodings().is_empty() {
                continue;
            }

            operation.add_response_header(
                kynos_openapi::StatusPattern::Code(status),
                name,
                &kynos_openapi::Header::new(kynos_openapi::Schema::of_type(
                    kynos_openapi::model::schema::types::SchemaType::String,
                ))
                .with_description(description),
            );
        }
    }
}

impl<C: Send + Sync + 'static> Endpoint<C> for AssetEndpoint {
    fn method(&self) -> Method {
        Method::Get
    }

    fn path(&self) -> &PathTemplate {
        &self.template
    }

    fn describe(&self, operation: &mut OperationCx<'_>) {
        operation.set_operation_id(&self.operation_id);
        operation.set_summary(format!("Serves {}", self.asset.path()));

        let mut responses = kynos_openapi::Responses::new().with(
            200,
            kynos_openapi::Response::with_content(
                "the file",
                self.asset.media_type(),
                // Unconstrained, as every binary codec describes its bytes.
                kynos_openapi::MediaType::new(kynos_openapi::Schema::Object(Box::default())),
            ),
        );

        // Reachable because the 200 carries an `ETag`.
        responses = responses.with(
            304,
            kynos_openapi::Response::new("the client's copy is current"),
        );

        // An `If-Match` naming a tag the file no longer carries.
        responses = responses.with(
            412,
            kynos_openapi::Response::new("the file is not the one the client's copy came from"),
        );

        operation.add_responses(&responses);

        // Read directly rather than extracted, but still declared.
        operation.add_parameter(
            kynos_openapi::Parameter::header(
                "If-Match",
                kynos_openapi::Schema::of_type(
                    kynos_openapi::model::schema::types::SchemaType::String,
                ),
            )
            .with_description(
                "The entity tag the client's copy was taken from, per RFC 9110 section 13.1.1",
            ),
        );
        operation.add_parameter(
            kynos_openapi::Parameter::header(
                "If-None-Match",
                kynos_openapi::Schema::of_type(
                    kynos_openapi::model::schema::types::SchemaType::String,
                ),
            )
            .with_description(
                "The entity tag the client already holds, per RFC 9110 section 13.1.2",
            ),
        );

        // Declared only where there is something to choose between.
        if !self.asset.encodings().is_empty() {
            let offered = self
                .asset
                .encodings()
                .iter()
                .map(super::Encoded::coding)
                .collect::<Vec<_>>()
                .join(", ");

            operation.add_parameter(
                kynos_openapi::Parameter::header(
                    "Accept-Encoding",
                    kynos_openapi::Schema::of_type(
                        kynos_openapi::model::schema::types::SchemaType::String,
                    ),
                )
                .with_description(format!(
                    "The content codings the client accepts, per RFC 9110 section 12.5.3. \
                     Stored for this file: {offered}"
                )),
            );
        }

        // A 206 carries the 200's representation fields (RFC 9110 section 15.3.7).
        range::describe(operation, self.asset.media_type());

        self.declare_response_headers(operation);
    }

    async fn call(&self, request: Request, context: &C) -> Response {
        let _ = context;

        let chosen = self.choose(request.headers());

        // Section 13.2.2 step 1, against the chosen form's tag.
        if let Some(refused) = range::precondition_failed(request.headers(), Some(chosen.etag)) {
            return refused;
        }

        // RFC 9110 section 13.1.2, against the tag of the representation this
        // request would receive, not one the client may no longer decode.
        if let Some(field) = request.headers().get(header::IF_NONE_MATCH) {
            if etag::matches(field, chosen.etag) {
                let mut response = Response::new(crate::http::body::Body::empty());
                *response.status_mut() = StatusCode::NOT_MODIFIED;
                crate::extract::params::header::write(
                    response.headers_mut(),
                    &self.headers(&chosen).not_modified(),
                );
                return response;
            }
        }

        // Section 14.2: `Range` only once a 200 is owed, guarded by the chosen
        // form's strong tag, since section 14.1.2 ranges over encoded octets.
        let requested = spec::read(request.method(), request.headers(), Some(chosen.etag));

        range::respond(
            bytes::Bytes::from_static(chosen.bytes),
            self.asset.media_type(),
            &self.headers(&chosen),
            &requested,
        )
    }
}

/// The form of an asset one request receives.
#[derive(Clone, Copy, Debug)]
struct Representation {
    bytes: &'static [u8],
    etag: &'static str,
    /// `None` for the identity octets, which carry no `Content-Encoding`.
    coding: Option<&'static str>,
}
