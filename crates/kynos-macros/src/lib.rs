//! Procedural macros for the Kynos REST API framework.
//!
//! Nothing here is meant to be used directly: every macro is re-exported from
//! `kynos`, and the documentation lives next to the trait each one implements.
//!
//! # Why the route attributes exist
//!
//! Kynos deliberately has no attribute DSL restating a handler's signature.
//! utoipa's `#[utoipa::path(responses(...))]` is written by hand beside the
//! code it describes, and nothing keeps the two in step — which is the single
//! most common way a generated OpenAPI document ends up wrong.
//!
//! The route attributes therefore carry only what the types cannot: the method,
//! the path, and prose. Parameters come from the handler's arguments, responses
//! from its return type, and neither is restated anywhere.
//!
//! What the attribute *does* add is compile-time checking the builder form
//! cannot do — chiefly that a path template's variables match the handler's
//! path parameters.
//!
//! # Why the examples here are `ignore`d
//!
//! Every expansion names `::kynos::…`, which this crate cannot depend on, so a
//! doctest here would not compile whatever the derive emitted. The compiled
//! demonstrations live in `crates/kynos/tests/derives.rs` and the framework's
//! examples; `AGENTS.md` records the carve-out.

#[cfg(feature = "assets")]
mod assets;
mod derive;
mod route;

use proc_macro::TokenStream;

/// Declares a `GET` operation.
///
/// ```ignore
/// /// Fetch a single user.
/// ///
/// /// The first line becomes the operation's summary, the rest its description.
/// #[kynos::get("/users/{id}", catch_panics)]
/// async fn get_user(Path(id): Path<UserId>) -> Result<Json<User>, ApiError> {
///     todo!()
/// }
/// ```
///
/// `catch_panics` installs a compile-time-selected recovery boundary for this
/// operation and contributes its 500 response. It is a compile-time error to
/// use it when the final binary is built with `panic = "abort"`.
///
/// Accepts `operation_id = "..."` and `tag = SomeTag` after the path. The tag
/// becomes `EndpointMeta::TAGS`, which is what puts it in the description; it
/// may be named once, since `Router::tag`, `Group::tag` and
/// `EndpointBuilder::tag` are how an operation acquires the rest.
#[proc_macro_attribute]
pub fn get(attribute: TokenStream, item: TokenStream) -> TokenStream {
    route::expand("GET", attribute, item)
}

/// Declares a `POST` operation. See [`macro@get`] for the syntax.
#[proc_macro_attribute]
pub fn post(attribute: TokenStream, item: TokenStream) -> TokenStream {
    route::expand("POST", attribute, item)
}

/// Declares a `PUT` operation. See [`macro@get`] for the syntax.
#[proc_macro_attribute]
pub fn put(attribute: TokenStream, item: TokenStream) -> TokenStream {
    route::expand("PUT", attribute, item)
}

/// Declares a `PATCH` operation. See [`macro@get`] for the syntax.
#[proc_macro_attribute]
pub fn patch(attribute: TokenStream, item: TokenStream) -> TokenStream {
    route::expand("PATCH", attribute, item)
}

/// Declares a `DELETE` operation. See [`macro@get`] for the syntax.
#[proc_macro_attribute]
pub fn delete(attribute: TokenStream, item: TokenStream) -> TokenStream {
    route::expand("DELETE", attribute, item)
}

/// Declares a `HEAD` operation. See [`macro@get`] for the syntax.
///
/// Rarely needed: a `HEAD` is answered from the corresponding `GET` unless one
/// is declared, per RFC 9110. Declare it only when the description should say
/// so explicitly.
#[proc_macro_attribute]
pub fn head(attribute: TokenStream, item: TokenStream) -> TokenStream {
    route::expand("HEAD", attribute, item)
}

/// Declares an `OPTIONS` operation. See [`macro@get`] for the syntax.
///
/// CORS preflight is handled without one: where a `Cors` interceptor covers a
/// path, the router registers a preflight answer on it while the service is
/// built. Declare this only for an `OPTIONS` that is part of the API's own
/// contract.
///
/// Declaring one *suppresses* the synthesized preflight on that path — a
/// hand-written operation wins, and it then owns answering preflights there too.
#[proc_macro_attribute]
pub fn options(attribute: TokenStream, item: TokenStream) -> TokenStream {
    route::expand("OPTIONS", attribute, item)
}

/// Declares a `TRACE` operation. See [`macro@get`] for the syntax.
#[proc_macro_attribute]
pub fn trace(attribute: TokenStream, item: TokenStream) -> TokenStream {
    route::expand("TRACE", attribute, item)
}

/// Declares a `QUERY` operation.
///
/// Requires the `openapi32` feature: `QUERY` has no Path Item field before
/// OpenAPI 3.2.
#[cfg(feature = "openapi32")]
#[proc_macro_attribute]
pub fn query(attribute: TokenStream, item: TokenStream) -> TokenStream {
    route::expand("QUERY", attribute, item)
}

/// Declares an operation for a method with no dedicated attribute.
///
/// ```ignore
/// #[kynos::operation(method = "PROPFIND", path = "/files/{id}")]
/// async fn propfind(Path(id): Path<FileId>) -> Result<Json<Properties>, ApiError> {
///     todo!()
/// }
/// ```
///
/// A method outside the eight OpenAPI 3.1 names emits into
/// `additionalOperations`, which requires the `openapi32` feature.
#[proc_macro_attribute]
pub fn operation(attribute: TokenStream, item: TokenStream) -> TokenStream {
    route::expand_generic(attribute, item)
}

/// Collects operations for mounting.
///
/// Operations sharing a path are merged into one Path Item, so `routes![list,
/// create]` on `/users` produces a single entry with `get` and `post`.
///
/// ```ignore
/// Router::new().mount(routes![users::list, users::create, users::get]);
/// ```
#[proc_macro]
pub fn routes(input: TokenStream) -> TokenStream {
    route::routes::expand_routes(input)
}

/// A path template validated at compile time.
///
/// ```ignore
/// let template = path!("/users/{id}");
/// ```
///
/// Rejects a template that does not start with `/`, has unbalanced braces,
/// repeats a variable, or carries a query string — none of which are legal as
/// a Paths key.
#[proc_macro]
pub fn path(input: TokenStream) -> TokenStream {
    route::path::expand_path(input)
}

/// Compiles a directory into the binary as a described asset set.
///
/// ```ignore
/// kynos::assets! {
///     /// The built single-page app.
///     pub struct Site;
///     dir = "dist",
///     exclude = [".map"],
///     warn_over = "4MiB",
/// }
/// ```
///
/// `dir` resolves against `CARGO_MANIFEST_DIR`. Every file becomes an
/// `include_bytes!`, so changing one rebuilds the crate; *adding* or *removing*
/// one does not, which a `cargo::rerun-if-changed` line in `build.rs` closes.
///
/// A file whose name no path template can express is a compile error naming it,
/// because a static asset Kynos cannot describe is one it will not serve.
/// Dotfiles and symlinks are skipped: the first keeps `.git` out of a binary,
/// the second keeps a set inside its own directory.
///
/// An embedded set past 2 MiB emits a compiler warning at the `dir` literal.
/// Raise the threshold with `warn_over = "8MiB"`, or turn it off with
/// `warn_over = "none"`. Under `-D warnings` it becomes an error, which is
/// arguably right and is exactly why the override exists.
#[cfg(feature = "assets")]
#[proc_macro]
pub fn assets(item: TokenStream) -> TokenStream {
    assets::expand(item)
}

/// Describes a type as JSON Schema.
///
/// Reads the serde attributes already on the type — `rename_all`, `skip`,
/// `flatten`, `alias`, `tag`, `content`, `transparent`, `deny_unknown_fields` —
/// so the schema and the wire form come from one declaration. A named field
/// serde reads under an `alias` is a property under each name it reads, present
/// under exactly one where it is required and under at most one otherwise,
/// since serde refuses a document naming two. A variant serde reads under an
/// `alias` is named under each name it reads: in the `enum` of an all-unit
/// enum, in the `enum` a tag or an externally tagged unit variant then is in
/// place of a `const`, and as a property of an externally tagged object branch,
/// present under exactly one. A name two variants claim is named under the
/// first alone, the one serde reads it as. Under
/// `deny_unknown_fields`, every object serde then refuses unknown keys in is
/// closed: a struct, each struct variant's fields, and an adjacently tagged
/// branch. The derive uses `additionalProperties: false`, or
/// `unevaluatedProperties: false` where the object carries an `allOf`, which a
/// flattened field composes members through and an aliased field bounds its
/// names in. An externally tagged branch that is an object admits
/// only its variant key, with or without the attribute, since serde reads it as
/// exactly one entry. A shape that closes does not implement `Flatten`, and
/// neither does any externally tagged enum. A field is left out of `required` when it is an
/// `Option`, carries `#[serde(default)]`, or belongs to a struct carrying
/// `#[serde(default)]`, because the wire form then allows it to be absent both
/// ways; `skip_serializing_if` is accepted only alongside one of those. A named
/// field serde reads and never writes, `skip_serializing` alone, is described
/// under that same rule, and one serde writes and never reads,
/// `skip_deserializing` alone, is left out. A `transparent` struct is described by the one field serde writes and reads
/// through, or the single field of the one direction serde can derive for it,
/// keeps its own component name, as a newtype does, and carries that field's
/// constraints. A tuple is the array of the members serde does not skip both
/// ways, and a newtype variant whose member serde skips is the unit variant
/// serde writes, provided serde also reads it back. A newtype, and each
/// described member of a tuple, tuple variant or newtype variant, carries its
/// constraints, prose and `#[deprecated]` as a named field does. A variant
/// serde skips both ways is described nowhere, and one serde reads and never
/// writes is described as serde reads it, under its prose and `#[deprecated]`.
///
/// Constraints go on fields, named or unnamed, and the grammar is exactly the
/// keys of
/// [`Constraints`](https://docs.rs/kynos/latest/kynos/schema/constraints/struct.Constraints.html)
/// so that
/// the attribute and the type it fills cannot drift: `minimum`, `maximum`,
/// `exclusive_minimum`, `exclusive_maximum`, `multiple_of`, `min_length`,
/// `max_length`, `pattern`, `min_items`, `max_items` and the `unique_items`
/// flag. They become JSON Schema assertions *and* the parser's checks, which is
/// what keeps the description honest without a JSON Schema interpreter on the
/// hot path.
///
/// `format` is **not** among them. It states what a value *is*, which follows
/// from the type or from nothing, so a `String` annotated as a UUID is a
/// compile error naming the remedy — `uuid::Uuid`, one of the date, time or
/// decimal types behind their features, or a newtype with its own `Schema`.
/// A constraint on one field is `pattern`; a claim about a type is the type's.
///
/// `open` is the one member of `#[schema(...)]` that is not a constraint. It
/// goes on a `#[serde(flatten)]` field to say that the object really does admit
/// members nothing names, which is the only thing a flattened map can mean. The
/// field's type must implement
/// [`OpenMap`](https://docs.rs/kynos/latest/kynos/schema/trait.OpenMap.html) — a
/// `HashMap`, a `BTreeMap` or an `Unchecked` over a map, not a type that refers
/// to one — and a key type's `propertyNames` does not survive it.
///
/// # Rejected, because serde and the schema would disagree
///
/// - `#[serde(with = ...)]`, `serialize_with`, `deserialize_with` on a field or
///   a variant. The wire form no longer follows from the Rust type, so a schema
///   derived from the Rust type would be a lie. Give the value a newtype whose
///   own `Serialize`, `Deserialize` and `Schema` agree on that form instead.
/// - `#[serde(untagged)]` enums. `anyOf` with no discriminator is ambiguous to
///   decode, and the tie-break is inexpressible. Use an internally or
///   adjacently tagged enum, which becomes a `discriminator`. The same holds for
///   one untagged variant serde reads or writes: it goes on the wire as its bare
///   payload, tried only after every tagged variant fails. On a variant serde
///   skips both ways it is in no schema, and is accepted.
/// - `#[serde(flatten)]` onto a field whose schema names none of its members,
///   which a map's does not. Its `additionalProperties` is defined against the
///   `properties` of its own schema object, and composing it into the parent's
///   `allOf` leaves it none — so the map's *value* schema would apply to the
///   properties the parent declared itself. Refused by a
///   [`Flatten`](https://docs.rs/kynos/latest/kynos/schema/trait.Flatten.html)
///   bound the expansion asserts per flattened field. `#[schema(open)]` on that
///   field is the opt-in that keeps the map: it says the object really is open,
///   and the map's values become the parent's `unevaluatedProperties`, the one
///   keyword that sees annotations across an `allOf`. The same bound holds the
///   payload of an internally tagged enum's newtype variant, which is composed
///   beside the tag in an `allOf` the same way.
/// - A `#[serde(other)]` catch-all on a variant serde reads, which only 3.2's
///   `defaultMapping` could describe and the derive does not emit. On a variant
///   serde skips both ways it catches nothing, and is accepted.
/// - `skip_serializing_if` on a non-`Option` field with no `#[serde(default)]`
///   on the field or its struct, and `#[serde(skip_serializing)]` alone on such
///   a field. serde may leave the field out of what it writes but still
///   requires it on read, so no `required` list is true in both directions.
///   Add `#[serde(default)]` beside it or on the struct. A
///   flattened field is decided by `#[schema(open)]` alone, since serde ignores
///   any default on it: an open map is exempt, and anything else is refused.
/// - `#[serde(into = ...)]`, `from` or `try_from` on the type itself, struct or
///   enum. serde then writes or reads the type they name, which the declared
///   fields or variants no longer predict. Implement `Schema` by hand, describing
///   the type serde converts through. `#[serde(remote = ...)]` is accepted,
///   since its fields mirror the type it names.
/// - `#[serde(transparent)]` on a struct serde writes through one field and
///   reads through another. serde writes through the field without `skip` or
///   `skip_serializing` and reads through the field without `skip`,
///   `skip_deserializing` or a field-level `default`; where each direction picks
///   a single field, the two must be the same. A struct where only one direction
///   picks a single field is described by it, since serde refuses the other
///   derive itself. Mark every other field `#[serde(skip)]`.
/// - `#[serde(skip_serializing)]` or `skip_deserializing` alone on a member of a
///   tuple struct, a tuple variant or a newtype variant, or `skip_serializing_if`
///   on a tuple member other than the last one under a `#[serde(default)]` on it
///   or its struct. A position, or a variant's payload, is there or not as a
///   whole, so serde would write one shape and read another. `#[serde(skip)]`
///   leaves the member out both ways; a newtype struct is exempt, since serde
///   ignores all three there.
/// - `#[serde(skip)]` on the only member of an adjacently tagged newtype variant,
///   unless that member is an `Option`. serde writes the variant as its tag
///   alone but reads it only with its content, so no one schema is true of
///   both. Make the member an `Option`, or skip the whole variant.
/// - `#[serde(skip_deserializing)]` alone on a variant. serde writes the variant
///   and refuses to read it back, so a closed `oneOf` or `enum` listing it
///   describes a request serde refuses, and one leaving it out describes a
///   response serde writes. `#[serde(skip)]` leaves the variant out both ways.
///   `skip_serializing` alone on a variant is accepted, since every variant serde
///   writes is one it reads.
/// - `#[serde(skip_deserializing)]` without `skip_serializing` on a named field
///   of an object that `deny_unknown_fields` closes, or beside a
///   `#[schema(open)]` flattened field whose type does not implement
///   [`AdmitsAny`](https://docs.rs/kynos/latest/kynos/schema/trait.AdmitsAny.html),
///   which a map, whose value schema is hoisted, does not unless its values are
///   `Unchecked`, and an `Unchecked` map does. serde writes the field and never
///   reads it, so the schema leaves it out, and the object then refuses what
///   serde writes of it: through being closed, or through the value schema a
///   map hoists as `unevaluatedProperties`. `#[serde(skip)]` leaves the field
///   out both ways. For the same reason a
///   struct, or an internally tagged enum whose struct variant serde writes,
///   holding such a field does not implement `Flatten`.
/// - A flattened `#[schema(open)]` field beside `#[serde(deny_unknown_fields)]`.
///   serde refuses every key the object's fields do not name before the map
///   sees it, so it reads the map empty and writes members it would refuse to
///   read back. Drop one of the two.
/// - A variant whose own name, after `rename` and `rename_all`, an earlier
///   variant also reads, by its own name or an `alias`. serde reads a name as
///   the first variant claiming it, so the later variant goes on the wire under
///   a name that reads back as the earlier one. An `alias` an earlier variant
///   already claims is accepted and named under that variant alone, since serde
///   never reads it as the later one.
#[proc_macro_derive(Schema, attributes(schema))]
pub fn derive_schema(item: TokenStream) -> TokenStream {
    derive::schema::expand(item)
}

/// Maps an error type to RFC 9457 problem details.
///
/// ```ignore
/// #[derive(Debug, thiserror::Error, ApiError)]
/// #[problem(base = "https://errors.example.com/")]
/// enum StoreError {
///     #[error("no user with id {id}")]
///     #[problem(status = 404, title = "User not found")]
///     NotFound {
///         #[problem(extension)]
///         id: UserId,
///         trace: String,
///     },
///
///     #[error("that email is already registered")]
///     #[problem(status = 409)]
///     EmailTaken,
/// }
/// ```
///
/// `status` is required on every variant and must be between 400 and 599. A
/// struct declares its one status on the type instead; `base` always belongs on
/// the type, since it is the prefix every variant's type URI shares.
///
/// The error's `Display` supplies each problem's `detail`, which is why
/// `thiserror` is the expected companion — the `#[error("...")]` a Rust reader
/// sees is the sentence an API consumer receives. A type without a `Display`
/// is rejected at the derive rather than at the handler returning it.
///
/// A field is published as an extension member only when it says
/// `#[problem(extension)]`, because a variant carries whatever the error site
/// had to hand and the default must not be to put that on the wire.
///
/// Also emits the `IntoResponse` and `Responses` implementations, so the
/// statuses the error can produce and the statuses the description advertises
/// cannot diverge. It is the only supported way to implement
/// `IntoProblem`.
#[proc_macro_derive(ApiError, attributes(problem))]
pub fn derive_api_error(item: TokenStream) -> TokenStream {
    derive::api_error::expand(item)
}

/// Declares a closed set of responses, one variant per status.
///
/// ```ignore
/// #[derive(Reply)]
/// enum CreateReply {
///     #[reply(status = 201, description = "the user as stored")]
///     Created(User),
///
///     #[reply(status = 200, description = "an identical user already existed")]
///     AlreadyExists(User),
/// }
/// ```
///
/// For an operation with more than one success shape. Modelled on
/// poem-openapi's `ApiResponse`, which is the best existing treatment of this.
///
/// `status` is required on every variant and must be between 200 and 599: a 1xx
/// is an interim response, and a handler returns the final one. No two variants
/// may declare the same status, since the description keys a reply's variants
/// by status alone — that is what "one variant per status" means, and it is the
/// one place this derive is stricter than [`ApiError`](macro@ApiError), whose
/// variants carry a `detail` that tells two occurrences of a status apart.
///
/// A variant's fields are its response body, so a variant holds either nothing,
/// for the empty body, or exactly one type describing the body. An anonymous
/// record has no name to register a component under.
#[proc_macro_derive(Reply, attributes(reply))]
pub fn derive_reply(item: TokenStream) -> TokenStream {
    derive::reply::expand(item)
}

/// Declares a group of path parameters.
///
/// Field names must match the route template's variables; the route attribute
/// emits a const assertion comparing the two sets.
#[proc_macro_derive(PathParams, attributes(param))]
pub fn derive_path_params(item: TokenStream) -> TokenStream {
    derive::path_params::expand(item)
}

/// Declares a group of query parameters.
///
/// Rejects a nested object: `deepObject` is defined only for objects whose
/// properties are scalars, so anything deeper has no legal serialization. The
/// diagnostic points at `QueryString<T, M>`, which describes such a shape
/// properly under `openapi32`.
#[proc_macro_derive(QueryParams, attributes(param))]
pub fn derive_query_params(item: TokenStream) -> TokenStream {
    derive::query_params::expand(item)
}

/// Declares a group of request or response headers.
///
/// Rejects `Accept`, `Content-Type` and `Authorization`. The specification says
/// a parameter definition for those is ignored; `Content-Type` is likewise
/// derived from a response's content map. Repeated fields such as `Set-Cookie`
/// remain separate header values rather than being comma joined. The
/// diagnostic names the right tool for each reserved field.
#[proc_macro_derive(HeaderParams, attributes(header))]
pub fn derive_headers(item: TokenStream) -> TokenStream {
    derive::headers::expand(item)
}

/// Declares a group of request cookies.
#[proc_macro_derive(CookieParams, attributes(cookie))]
pub fn derive_cookies(item: TokenStream) -> TokenStream {
    derive::cookies::expand(item)
}

/// Declares the fields of a `multipart/form-data` body, in both directions.
///
/// ```ignore
/// #[derive(Schema, MultipartForm)]
/// struct Upload {
///     name: String,
///     caption: Option<String>,
///     images: Vec<FilePart>,
/// }
/// ```
///
/// One declaration, two implementations: `FromMultipart` reads each field from
/// the part carrying its name, and `IntoMultipart` writes it back under the
/// same one — so a body `MultipartForm<T>` accepts is a body it can produce.
///
/// A field's type says how many parts carry it: `Vec<T>` is one part per
/// element, `Option<T>` is a part that need not have been sent, and anything
/// else is a part that must have been, whose second and later occurrences are
/// ignored. The element type converts through `FromPart` and `IntoPart`, which
/// Kynos implements for `FilePart`, `String` and `Bytes`.
///
/// A part naming no declared field is ignored, since a form may carry what the
/// agent rendering it added.
///
/// There is no attribute of its own. Derive [`Schema`](macro@Schema) alongside:
/// this derive says how the parts travel and `Schema` is what puts them in the
/// description, and both read the part names from the same place — the field's
/// identifier, or serde's `rename` and `rename_all` when the type carries them.
#[proc_macro_derive(MultipartForm)]
pub fn derive_multipart_form(item: TokenStream) -> TokenStream {
    derive::multipart::expand(item)
}

/// Declares a tag.
///
/// ```ignore
/// #[derive(Tag)]
/// #[tag(name = "users", description = "Managing user accounts")]
/// struct Users;
/// ```
///
/// Tags are types rather than strings, so a typo is a compile error and
/// uniqueness follows from the module system. `#[tag(parent = Admin)]` nests
/// one tag under another, which requires `openapi32`.
#[proc_macro_derive(Tag, attributes(tag))]
pub fn derive_tag(item: TokenStream) -> TokenStream {
    derive::tag::expand(item)
}

/// Declares a security scheme.
///
/// ```ignore
/// #[derive(SecurityScheme)]
/// #[security(http, scheme = "bearer", bearer_format = "JWT")]
/// struct Bearer;
/// ```
#[proc_macro_derive(SecurityScheme, attributes(security))]
pub fn derive_security_scheme(item: TokenStream) -> TokenStream {
    derive::security_scheme::expand(item)
}

/// Declares an application context, emitting one `Provides` implementation per
/// field.
///
/// ```ignore
/// #[derive(Provider)]
/// struct App {
///     pool: Pool,
///     cache: Cache,
///     #[provide(skip)]
///     started_at: Instant,
/// }
/// ```
///
/// Each provided field's type must be `Clone`, since a value is handed out per
/// request; a handle is the intended shape. A handler asking for something no
/// field supplies fails to typecheck, rather than panicking at runtime the way
/// an erased state map does.
///
/// Two provided fields of the same type are rejected here, naming both, rather
/// than being left to produce a coherence error about the derive's own output.
#[proc_macro_derive(Provider, attributes(provide))]
pub fn derive_provider(item: TokenStream) -> TokenStream {
    derive::provider::expand(item)
}
