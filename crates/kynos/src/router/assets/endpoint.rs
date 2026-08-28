//! One described operation per file.

use kynos_openapi::{Method, PathTemplate};

use crate::{
    extract::params::header::HeaderParams,
    http::{HeaderValue, Request, Response, StatusCode, etag, header},
    response::range::spec,
    router::{
        assets::{Asset, range},
        endpoint::{Endpoint, operation_id},
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
    /// The coding the selected representation is in, if it is not identity.
    coding: Option<&'static str>,
    /// Whether this set has anything to negotiate over.
    ///
    /// `Vary` is sent by a resource that *could* answer differently, not only
    /// by a response that did: a cache storing the identity form of a file with
    /// stored codings must not serve it to a client that asked for `br`. A file
    /// with no stored coding answers the same way whatever is accepted, and
    /// sending `Vary` for it would partition a cache key for nothing.
    negotiable: bool,
}

impl AssetHeaders {
    /// The same group, as a 304 is allowed to carry it.
    ///
    /// RFC 9110 section 15.4.5 lists what a 304 *must* repeat from the 200 --
    /// `Content-Location`, `Date`, `ETag`, `Vary`, `Cache-Control` and
    /// `Expires` -- and then bounds the rest: "a sender SHOULD NOT generate
    /// representation metadata other than the above listed fields unless said
    /// metadata exists for the purpose of guiding cache updates".
    ///
    /// `Content-Encoding` is representation metadata and is not on that list.
    /// Nor does it guide a cache update: RFC 9111 section 4.3.4 identifies the
    /// stored response to update by its validator, and the `ETag` that goes
    /// with the 304 already names the coding, since each stored form carries
    /// its own. So the field is dropped rather than repeated -- which is also
    /// why `declare_response_headers` declares `Content-Encoding` for 200 and
    /// 206 only.
    fn not_modified(mut self) -> Self {
        self.coding = None;
        self
    }
}

impl HeaderParams for AssetHeaders {
    const NAMES: &'static [&'static str] = &["etag", "cache-control", "content-encoding", "vary"];

    // `VARIES` is deliberately not set. It is a constant on the group, so it
    // would put `Vary: Accept-Encoding` on *every* asset -- including the files
    // with one stored form, which answer the same way whatever is accepted and
    // would have their cache key partitioned for nothing. Whether this resource
    // negotiates is a property of the file, not of the type, so the field is
    // written per instance below.
    //
    // Merging is not lost by writing it here: an interceptor above this one
    // contributes its own `Vary` through `vary_on`, which merges into whatever
    // the response already carries -- and this endpoint is the innermost
    // writer, so there is never an inner value for it to clobber.

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

    /// The group both a success and a 304 carry, for the representation chosen.
    fn headers(&self, chosen: &Representation) -> AssetHeaders {
        AssetHeaders {
            etag: chosen.etag,
            cache_control: self.cache_control,
            coding: chosen.coding,
            negotiable: !self.asset.encodings().is_empty(),
        }
    }

    /// The representation this request gets.
    ///
    /// Selected before anything else is evaluated, because every answer below
    /// is *about* a representation: the tag `If-None-Match` is compared
    /// against, the tag `If-Range` is compared against, and the octets a byte
    /// range is calculated over all belong to the form actually being sent.
    /// Choosing afterwards is the mistake this whole feature exists to make
    /// impossible.
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
            // RFC 9110 section 12.5.3 rule 1: with no field, any coding is
            // acceptable -- but "acceptable" is not "wanted". A client that
            // said nothing gets the form every client can read.
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

    /// The response fields each status carries, declared only where they exist.
    ///
    /// Split from `describe` because the list outgrew one function, and because
    /// the two conditions are the part worth seeing: a field this set can never
    /// send must not be declared, or `assert_declared_responses_covered`
    /// reports a promise nothing keeps.
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
            // Both only exist where a coding was stored. A file with one form
            // never sends either, and declaring them would put a field in the
            // description no response can carry -- the exact gap
            // `assert_declared_responses_covered` exists to catch.
            //
            // Which is also why the two part company at 304: section 15.4.5
            // requires `Vary` there and bounds the representation metadata that
            // may join it, so `Content-Encoding` is neither sent nor declared.
            if (name == "Content-Encoding" || name == "Vary") && self.asset.encodings().is_empty() {
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

        // Read only where there is something to choose between. Declaring it on
        // a file with one stored form would describe a negotiation that cannot
        // change the answer.
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

        // A 206 carries what a 200 would have, which section 15.3.7 requires of
        // it outright: *a sender MUST generate all of the representation header
        // fields that would have been sent in a 200 (OK) response to the same
        // request.*
        range::describe(operation, self.asset.media_type());

        self.declare_response_headers(operation);
    }

    async fn call(&self, request: Request, context: &C) -> Response {
        let _ = context;

        // Which representation, first. Every condition below is about one.
        let chosen = self.choose(request.headers());

        // RFC 9110 section 13.1.2: `If-None-Match` on a GET is a cache
        // validation, and a match means the client's copy is current.
        //
        // Compared against the tag of the representation *this* request would
        // receive. A client holding the brotli form and now sending
        // `Accept-Encoding: identity` has a current copy of something it is no
        // longer being offered, and answering 304 would leave it with octets it
        // just said it cannot decode.
        if let Some(field) = request.headers().get(header::IF_NONE_MATCH) {
            if matches(field, chosen.etag) {
                let mut response = Response::new(crate::http::body::Body::empty());
                *response.status_mut() = StatusCode::NOT_MODIFIED;
                crate::extract::params::header::write(
                    response.headers_mut(),
                    // Without the `Content-Encoding`: section 15.4.5 bounds a
                    // 304 to the fields it lists, and the `ETag` below already
                    // names which stored form the client's copy is.
                    &self.headers(&chosen).not_modified(),
                );
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
        //
        // The entity tag goes with it, and it is the *chosen* representation's:
        // section 14.1.2 calculates a range against the encoded octets when a
        // coding is applied, so a range and the tag guarding it have to name the
        // same form. That is the property one tag over two representations
        // cannot have, and the reason each stored coding carries its own.
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
