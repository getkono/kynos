//! One described operation per file.

use kynos_openapi::{Method, PathTemplate};

use crate::{
    extract::params::header::HeaderParams,
    http::{HeaderValue, Request, Response, StatusCode, etag, header},
    response::range::spec,
    router::{
        assets::{Asset, range},
        endpoint::Endpoint,
        operation::OperationCx,
    },
};

/// The `ETag` an asset response carries.
///
/// A group rather than a bare insert, so the field is *declared* and the
/// conflict check sees it — an interceptor also setting `ETag` over an asset
/// mount is then a compile error rather than a response with two.
#[derive(Clone, Copy, Debug)]
struct AssetHeaders {
    etag: &'static str,
    cache_control: Option<&'static str>,
}

impl HeaderParams for AssetHeaders {
    const NAMES: &'static [&'static str] = &["etag", "cache-control"];

    fn encode(&self) -> Vec<(crate::http::HeaderName, HeaderValue)> {
        let mut fields = Vec::with_capacity(2);

        if let Ok(value) = HeaderValue::from_str(self.etag) {
            fields.push((header::ETAG, value));
        }
        if let Some(cache_control) = self.cache_control {
            if let Ok(value) = HeaderValue::from_str(cache_control) {
                fields.push((header::CACHE_CONTROL, value));
            }
        }

        fields
    }
}

/// One file, served at one path.
///
/// A path Kynos owns rather than one a request supplied: the set is enumerated
/// before the router is built, so nothing here joins request input onto
/// anything. That is what makes traversal unrepresentable rather than defended
/// against — there is no path to traverse.
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
    /// If `path` is not a legal path template. `assets!` refuses such a name at
    /// compile time and the filesystem walk refuses it while enumerating, so
    /// reaching this means a caller built an `Asset` by hand with a name Kynos
    /// cannot describe.
    #[must_use]
    pub(super) fn new(
        asset: Asset,
        path: String,
        cache_control: Option<&'static str>,
        prefix: &str,
    ) -> Self {
        let relative = format!("/{}", path.trim_start_matches('/'));
        let template = PathTemplate::parse(&relative)
            .unwrap_or_else(|error| panic!("`{relative}` is not a servable asset path: {error}"));

        Self {
            asset,
            template,
            operation_id: operation_id(prefix, &path),
            cache_control,
        }
    }

    /// The group both a success and a 304 carry.
    fn headers(&self) -> AssetHeaders {
        AssetHeaders {
            etag: self.asset.etag(),
            cache_control: self.cache_control,
        }
    }
}

/// A stable, readable identifier for one served path.
///
/// Derived from the path rather than counted, so two sets mounted in one router
/// collide only where they genuinely serve the same file — and so the id does
/// not move when a file is added beside it.
fn operation_id(prefix: &str, path: &str) -> String {
    let mut id = String::with_capacity(prefix.len() + path.len() + 1);
    id.push_str(prefix);

    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        id.push_str("_index");
        return id;
    }

    id.push('_');
    for character in trimmed.chars() {
        // An `operationId` is a token a generator turns into a function name,
        // so anything that is not one becomes `_`.
        if character.is_ascii_alphanumeric() {
            id.push(character);
        } else {
            id.push('_');
        }
    }
    id
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
        operation.set_summary(&format!("Serves {}", self.asset.path()));

        let mut responses = kynos_openapi::Responses::new().with(
            200,
            kynos_openapi::Response::with_content(
                "the file",
                self.asset.media_type(),
                // The same unconstrained object every binary codec describes: a
                // file's bytes have no JSON Schema, and one claiming otherwise
                // would be claiming more than it can check.
                kynos_openapi::MediaType::new(kynos_openapi::Schema::Object(Box::default())),
            ),
        );

        // 304 is reachable exactly because the 200 carries an `ETag`: a client
        // that received one can send it back. Declaring it without the
        // validator would be a status nothing could provoke.
        responses = responses.with(
            304,
            kynos_openapi::Response::new("the client's copy is current"),
        );

        operation.add_responses(responses);

        // `If-None-Match` is read, so it is declared. The group is not used for
        // extraction -- an asset endpoint reads it directly -- but a consumer
        // is entitled to know the request field exists.
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

        // A 206 carries what a 200 would have, which section 15.3.7 requires of
        // it outright: *a sender MUST generate all of the representation header
        // fields that would have been sent in a 200 (OK) response to the same
        // request.*
        range::describe(operation, self.asset.media_type());

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
        ] {
            if name == "Cache-Control" && self.cache_control.is_none() {
                continue;
            }

            operation.add_response_header(
                kynos_openapi::StatusPattern::Code(status),
                name,
                kynos_openapi::Header::new(kynos_openapi::Schema::of_type(
                    kynos_openapi::model::schema::types::SchemaType::String,
                ))
                .with_description(description),
            );
        }
    }

    async fn call(&self, request: Request, context: &C) -> Response {
        let _ = context;

        // RFC 9110 section 13.1.2: `If-None-Match` on a GET is a cache
        // validation, and a match means the client's copy is current.
        if let Some(field) = request.headers().get(header::IF_NONE_MATCH) {
            if matches(field, self.asset.etag()) {
                let mut response = Response::new(crate::http::body::Body::empty());
                *response.status_mut() = StatusCode::NOT_MODIFIED;
                crate::extract::params::header::write(response.headers_mut(), &self.headers());
                return response;
            }
        }

        // Section 14.2: the `Range` field *is evaluated after evaluating the
        // precondition header fields defined in Section 13.1, and only if the
        // result in absence of the Range header field would be a 200* — so the
        // 304 above wins, and this is reached only where a 200 was owed.
        //
        // The entity tag goes with it: `assets!` mints a strong one from the
        // file's contents, which is what lets section 13.1.5's `If-Range`
        // condition be evaluated rather than assumed false.
        let requested = spec::read(request.method(), request.headers(), Some(self.asset.etag()));

        range::respond(
            bytes::Bytes::from_static(self.asset.bytes()),
            self.asset.media_type(),
            &self.headers(),
            &requested,
        )
    }
}

/// Whether `field` names `current`, per RFC 9110 section 13.1.2.
///
/// `*` matches anything the server has. Otherwise the field is a
/// `1#entity-tag` and the *weak* comparison applies — `W/"x"` and `"x"` are the
/// same representation for a cache validation, which is the whole point of
/// `If-None-Match`.
///
/// Both halves come from [`http::etag`](crate::http::etag), which is the one
/// place in the crate that knows a comma can sit inside an `opaque-tag`.
pub(super) fn matches(field: &HeaderValue, current: &str) -> bool {
    let Ok(text) = field.to_str() else {
        return false;
    };

    if text.trim() == etag::ANY {
        return true;
    }

    etag::split(text).any(|candidate| etag::weak_match(candidate, current))
}
