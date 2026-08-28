//! Describing one operation while the router is built.

use kynos_openapi::{ComponentName, Method, RefOr, Response, StatusPattern};

use crate::{http::StatusCode, schema::registry::Registry};

/// The operation a request matched.
///
/// Handed to an interceptor while the router is built, and to an interceptor or
/// observer while a request is served, so that a metric label, a log field or a
/// rate-limit bucket can be keyed by the operation rather than by the raw path.
/// That is what keeps label cardinality bounded — and because
/// [`path`](Route::path) is the same string that appears as the `paths` key,
/// the label cannot disagree with the description.
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
}

impl<'a> OperationCx<'a> {
    /// Begins describing an operation against `registry`.
    #[must_use]
    pub fn new(registry: &'a mut Registry) -> Self {
        Self {
            registry,
            operation: kynos_openapi::Operation::default(),
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
    pub fn add_parameter(&mut self, parameter: kynos_openapi::Parameter) {
        let declared = self.operation.parameters.iter().any(|existing| {
            matches!(
                existing,
                RefOr::Item(item)
                    if item.name == parameter.name && item.location == parameter.location
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

    /// Adds a security requirement.
    ///
    /// Repeating a requirement already declared is a no-op: a list of
    /// requirements is satisfied when any one of them is, so a duplicate adds
    /// nothing but noise.
    pub fn add_security(&mut self, requirement: kynos_openapi::SecurityRequirement) {
        let declared = self.operation.security.get_or_insert_with(Vec::new);
        if !declared.contains(&requirement) {
            declared.push(requirement);
        }
    }

    /// Registers a security scheme under `components`.
    ///
    /// Idempotent for the same scheme under the same name. Two different
    /// schemes under one name are recorded and reported when the router is
    /// built, because a [`Describe`](crate::extract::describe::Describe)
    /// implementation has no way to return an error.
    ///
    /// Without this an `Auth<S>` argument could require a credential it had no
    /// way to declare, and every operation using one would emit a security
    /// requirement naming a scheme the document never defines.
    pub fn add_security_scheme(
        &mut self,
        name: ComponentName,
        scheme: kynos_openapi::SecurityScheme,
    ) {
        self.registry.declare_security_scheme(name, scheme);
    }

    /// Merges responses an input's rejection can produce.
    ///
    /// What the operation already declares wins: an input describes what its
    /// own rejection looks like, and the handler's own response for a status is
    /// the more specific of the two. Only statuses the operation says nothing
    /// about are taken from `responses`.
    pub fn add_responses(&mut self, responses: kynos_openapi::Responses) {
        self.operation.responses.merge_from(&responses);
    }

    /// Declares a header this input causes the operation to send.
    ///
    /// `WWW-Authenticate` on a 401 is the motivating case: the challenge is
    /// part of what a client must handle, and only the scheme knows it.
    ///
    /// The response filed under `status` gains the header, and is created with
    /// a generic description if the operation does not declare one yet — the
    /// header is evidence the response happens, so omitting it would be worse
    /// than describing it thinly. A header already declared under that name is
    /// left alone, and a response held as a `$ref` is not reached into.
    ///
    /// A *range* pattern never mints its own entry. It reaches the responses
    /// the operation already declares within it, and contributes nothing when
    /// there are none.
    ///
    /// Both halves fix the same thing. The specification gives a consumer
    /// resolving a status the exact key first, so a header filed under `2XX`
    /// beside a declared `200` is one no reader of that operation's 200 will
    /// ever find — and the minted `2XX` is then a response the service cannot
    /// produce, which is a claim in the description that nothing can keep. For
    /// an operation declaring no success at all — a redirect — the wildcard
    /// would be the same untruth with nothing beside it, so the header is
    /// dropped instead. That understates the description by one header on a
    /// response that has one; the alternative overstates it by a response that
    /// does not exist, and `nfr.md`'s *emitted ⊇ observable* is the direction
    /// that must not break.
    pub fn add_response_header(
        &mut self,
        status: StatusPattern,
        name: impl Into<String>,
        header: kynos_openapi::Header,
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
                declare_header(
                    &mut self.operation.responses.responses,
                    &key,
                    &name,
                    &header,
                );
            }

            return;
        }

        let key = status.to_string();
        self.operation
            .responses
            .responses
            .entry(key.clone())
            .or_insert_with(|| RefOr::Item(Response::new(describe_status(status))));
        declare_header(
            &mut self.operation.responses.responses,
            &key,
            &name,
            &header,
        );
    }

    /// Sets the operation identifier.
    pub fn set_operation_id(&mut self, id: &str) {
        self.operation.operation_id = Some(id.to_owned());
    }

    /// Sets the summary.
    pub fn set_summary(&mut self, summary: &str) {
        self.operation.summary = Some(summary.to_owned());
    }

    /// Sets the description.
    pub fn set_description(&mut self, description: &str) {
        self.operation.description = Some(description.to_owned());
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
    /// Adding a tag the operation already carries is a no-op: `tags` is a set
    /// spelled as an array, and a repeated entry names no further group.
    pub fn add_tag(&mut self, name: &str) {
        if !self.operation.tags.iter().any(|tag| tag == name) {
            self.operation.tags.push(name.to_owned());
        }
    }

    /// The registry, for describing a schema this input needs.
    pub fn registry(&mut self) -> &mut Registry {
        self.registry
    }
}

/// The description given to a response entry created only to carry a header.
///
/// A `Response` must have one, and the reason phrase RFC 9110 registers for the
/// status is the most any caller has said about it.
/// Whether `declared` names responses `range` covers.
///
/// An exact code is covered when the range matches it; an identical range is
/// covered by itself, so a second contribution under `2XX` still lands on the
/// `2XX` a first one created rather than beside it.
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
/// Derived with `#[derive(Tag)]` on a unit struct. Making tags types rather
/// than strings means a typo is a compile error, and tag-name uniqueness is a
/// property of the module system rather than something checked afterwards.
pub trait Tag {
    /// The tag name as it appears in the description.
    const NAME: &'static str;

    /// The tag's metadata.
    fn metadata() -> kynos_openapi::Tag;
}
