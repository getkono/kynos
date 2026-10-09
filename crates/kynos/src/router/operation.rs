//! Describing one operation while the router is built.

use kynos_openapi::{ComponentName, Method, RefOr, Response, StatusPattern};

use crate::{http::StatusCode, schema::registry::Registry};

/// The operation a request matched.
///
/// Handed to an interceptor while the router is built, and to an interceptor or
/// observer while a request is served, so that a metric label, a log field or a
/// rate-limit bucket can be keyed by the operation rather than by the raw path,
/// with bounded cardinality. [`path`](Route::path) is the `paths` key itself.
///
/// Borrowed and [`Copy`]: nothing here allocates on the request path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Route<'a> {
    path: &'a str,
    operation_id: &'a str,
    method: Method,
}

impl<'a> Route<'a> {
    /// Names an operation.
    pub(crate) fn new(path: &'a str, operation_id: &'a str, method: Method) -> Self {
        Self {
            path,
            operation_id,
            method,
        }
    }

    /// The `paths` key this request matched, exactly as the description spells
    /// it — with its `{}` expressions intact, never the request's own path.
    #[must_use]
    pub fn path(&self) -> &'a str {
        self.path
    }

    /// The operation identifier.
    #[must_use]
    pub fn operation_id(&self) -> &'a str {
        self.operation_id
    }

    /// The method.
    #[must_use]
    pub fn method(&self) -> Method {
        self.method
    }
}

/// The description of the operation currently being built.
///
/// Passed to [`Describe`](crate::extract::describe::Describe) implementations
/// so that each handler input can add its own parameters or request body, and
/// to [`Handler::describe`](crate::handler::Handler::describe), which assembles
/// the whole operation from them.
#[derive(Debug)]
pub struct OperationCx<'a> {
    registry: &'a mut Registry,
    operation: kynos_openapi::Operation,
    /// What the operation's `tags` names came from, in the order they landed,
    /// so the describe walk registers metadata for exactly the tags it carries.
    tags: Vec<DeclaredTag>,
}

impl<'a> OperationCx<'a> {
    /// Begins describing an operation against `registry`.
    #[must_use]
    pub fn new(registry: &'a mut Registry) -> Self {
        Self {
            registry,
            operation: kynos_openapi::Operation::default(),
            tags: Vec::new(),
        }
    }

    /// Finishes the operation being described.
    #[must_use]
    pub fn finish(self) -> kynos_openapi::Operation {
        self.operation
    }
}

impl OperationCx<'_> {
    /// Adds a parameter to the operation.
    ///
    /// A parameter is identified by its name and location, and the first
    /// declaration of a given pair wins: two inputs describing the same query
    /// parameter contribute one entry, not a duplicate the specification
    /// forbids.
    ///
    /// A *header* name is compared case-insensitively (RFC 9110 section 5.1);
    /// every other location is compared as written.
    pub fn add_parameter(&mut self, parameter: kynos_openapi::Parameter) {
        let names_the_same_field = |existing: &kynos_openapi::Parameter| {
            if parameter.location == kynos_openapi::ParameterIn::Header {
                existing.name.eq_ignore_ascii_case(&parameter.name)
            } else {
                existing.name == parameter.name
            }
        };

        let declared = self.operation.parameters.iter().any(|existing| {
            matches!(
                existing,
                RefOr::Item(item)
                    if names_the_same_field(item) && item.location == parameter.location
            )
        });

        if !declared {
            self.operation.parameters.push(RefOr::Item(parameter));
        }
    }

    /// Sets the operation's request body.
    ///
    /// # Panics
    ///
    /// Panics if a request body was already set. The trait bounds make this
    /// unreachable from a handler — only one argument may implement
    /// [`FromRequest`](crate::extract::FromRequest) — so reaching it indicates
    /// a hand-written [`Describe`](crate::extract::describe::Describe)
    /// implementation that claims a body it does not consume.
    pub fn set_request_body(&mut self, body: kynos_openapi::RequestBody) {
        assert!(
            self.operation.request_body.is_none(),
            "the operation already declares a request body; only one handler argument may \
             implement `FromRequest`, so a second one comes from a hand-written `Describe`"
        );
        self.operation.request_body = Some(RefOr::Item(body));
    }

    /// Sets the operation's security: the whole list of alternatives, once.
    ///
    /// # Panics
    ///
    /// Panics if already set: a handler takes one [`Guard`](crate::security::Guard).
    pub fn set_security(&mut self, requirements: Vec<kynos_openapi::SecurityRequirement>) {
        let previous = self.operation.security.replace(requirements);
        assert!(previous.is_none(), "its security is already set");
    }

    /// Registers a security scheme under `components`.
    ///
    /// Idempotent for the same scheme under the same name. Two different
    /// schemes under one name are recorded and reported when the router is
    /// built, because a [`Describe`](crate::extract::describe::Describe)
    /// implementation has no way to return an error.
    pub fn add_security_scheme(
        &mut self,
        name: ComponentName,
        scheme: kynos_openapi::SecurityScheme,
    ) {
        self.registry.declare_security_scheme(name, scheme);
    }

    /// Merges responses a contributor to this operation can produce.
    ///
    /// A status the operation does not declare is taken from `responses`; one
    /// it does keeps its entry, unless both are problem documents narrowed to
    /// the types they publish, which
    /// [`union_from`](kynos_openapi::Responses::union_from) joins into a choice
    /// over both rather than letting contribution order decide.
    pub fn add_responses(&mut self, responses: &kynos_openapi::Responses) {
        self.operation.responses.union_from(responses);
    }

    /// Declares a header this input causes the operation to send.
    ///
    /// `WWW-Authenticate` on a 401 is the motivating case: the challenge is
    /// part of what a client must handle, and only the scheme knows it.
    ///
    /// The response filed under `status` gains the header, and is created with
    /// a generic description if the operation does not declare one yet. A
    /// header already declared under that name is left alone, and a response
    /// held as a `$ref` is not reached into.
    ///
    /// A *range* pattern never mints its own entry. It reaches the responses
    /// the operation already declares within it, and contributes nothing when
    /// there are none, since a minted `2XX` would describe a response the
    /// service cannot produce.
    pub fn add_response_header(
        &mut self,
        status: StatusPattern,
        name: impl Into<String>,
        header: &kynos_openapi::Header,
    ) {
        let name = name.into();

        if status.is_range() {
            let covered: Vec<String> = self
                .operation
                .responses
                .responses
                .keys()
                .filter(|key| {
                    key.parse::<StatusPattern>()
                        .is_ok_and(|declared| covered_by(status, declared))
                })
                .cloned()
                .collect();

            for key in covered {
                declare_header(&mut self.operation.responses.responses, &key, &name, header);
            }

            return;
        }

        let key = status.to_string();
        self.operation
            .responses
            .responses
            .entry(key.clone())
            .or_insert_with(|| RefOr::Item(Response::new(describe_status(status))));
        declare_header(&mut self.operation.responses.responses, &key, &name, header);
    }

    /// Sets the operation identifier.
    pub fn set_operation_id(&mut self, id: impl Into<String>) {
        self.operation.operation_id = Some(id.into());
    }

    /// Sets the summary.
    pub fn set_summary(&mut self, summary: impl Into<String>) {
        self.operation.summary = Some(summary.into());
    }

    /// Sets the description.
    pub fn set_description(&mut self, description: impl Into<String>) {
        self.operation.description = Some(description.into());
    }

    /// Marks the operation deprecated.
    ///
    /// `false` leaves the field out rather than stating the default, so a
    /// description never carries `deprecated: false`.
    pub fn set_deprecated(&mut self, deprecated: bool) {
        self.operation.deprecated = deprecated.then_some(true);
    }

    /// Adds a tag.
    ///
    /// Adding a tag the operation already carries is a no-op, so a tag named
    /// at two scopes keeps the slot of the *first* to name it.
    /// `docs/routing.md` carries the full order.
    ///
    /// A [`DeclaredTag`] rather than a name, so that the metadata documenting
    /// the tag arrives with it.
    pub fn add_tag(&mut self, tag: DeclaredTag) {
        if !self.operation.tags.iter().any(|name| name == tag.name()) {
            self.operation.tags.push(tag.name().to_owned());
            self.tags.push(tag);
        }
    }

    /// The tags that landed on this operation, for the document to document.
    pub(crate) fn declared_tags(&self) -> &[DeclaredTag] {
        &self.tags
    }

    /// The registry, for describing a schema this input needs.
    pub fn registry(&mut self) -> &mut Registry {
        self.registry
    }
}

/// Whether `declared` names responses `range` covers: a matching code, or the
/// identical range.
fn covered_by(range: StatusPattern, declared: StatusPattern) -> bool {
    match declared {
        StatusPattern::Code(code) => range.matches(code),
        other => other == range,
    }
}

/// Files `header` under `name` on the response at `key`, leaving a name already
/// declared alone and never reaching into a `$ref`.
fn declare_header(
    responses: &mut kynos_openapi::Map<RefOr<kynos_openapi::Response>>,
    key: &str,
    name: &str,
    header: &kynos_openapi::Header,
) {
    if let Some(RefOr::Item(response)) = responses.get_mut(key) {
        response
            .headers
            .entry(name.to_owned())
            .or_insert_with(|| RefOr::Item(header.clone()));
    }
}

/// The description given to a response entry created only to carry a header:
/// the status's RFC 9110 reason phrase.
fn describe_status(status: StatusPattern) -> String {
    let class = match status {
        StatusPattern::Code(code) => {
            return StatusCode::from_u16(code)
                .ok()
                .and_then(|code| code.canonical_reason())
                .map_or_else(|| format!("a `{code}` response"), str::to_owned);
        }
        StatusPattern::Informational => "informational",
        StatusPattern::Success => "successful",
        StatusPattern::Redirection => "redirection",
        StatusPattern::ClientError => "client error",
        StatusPattern::ServerError => "server error",
    };

    format!("a {class} response")
}

/// A tag, as a type.
///
/// Derived with `#[derive(Tag)]` on a unit struct, so a typo is a compile
/// error.
pub trait Tag {
    /// The tag name as it appears in the description.
    const NAME: &'static str;

    /// The tag's metadata.
    fn metadata() -> kynos_openapi::Tag;
}

/// A tag as a scope declared it: its name, and the thunk that documents it.
///
/// [`Copy`] and const-constructible, so a route attribute puts one in an
/// associated constant of
/// [`EndpointMeta`](crate::router::endpoint::meta::EndpointMeta).
#[derive(Clone, Copy, Debug)]
pub struct DeclaredTag {
    name: &'static str,
    metadata: fn() -> kynos_openapi::Tag,
}

impl DeclaredTag {
    /// The declaration of `T`.
    #[must_use]
    pub const fn of<T: Tag>() -> Self {
        Self {
            name: T::NAME,
            metadata: T::metadata,
        }
    }

    /// The name, as it appears in the operation's `tags`.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The metadata, as it appears in the document's `tags`.
    #[must_use]
    pub fn metadata(&self) -> kynos_openapi::Tag {
        (self.metadata)()
    }
}
