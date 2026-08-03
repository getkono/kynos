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
/// Accepts `operation_id = "..."` and `tag = SomeTag` after the path.
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
/// CORS preflight is handled without one; declare this only for an `OPTIONS`
/// that is part of the API's own contract.
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
    route::expand_routes(input)
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
    route::expand_path(input)
}

/// Describes a type as JSON Schema.
///
/// Reads the serde attributes already on the type — `rename_all`, `skip`,
/// `flatten`, `tag`, `content` — so the schema and the wire form come from one
/// declaration.
///
/// Constraints go on fields as `#[schema(min = 1, max = 100)]`,
/// `#[schema(pattern = "...")]`, `#[schema(format = "uuid")]`. They become JSON
/// Schema assertions *and* the parser's checks, which is what keeps the
/// description honest without a JSON Schema interpreter on the hot path.
///
/// # Rejected, because serde and the schema would disagree
///
/// - `#[serde(with = ...)]`, `serialize_with`, `deserialize_with` on a field.
///   The wire form no longer follows from the Rust type, so a schema derived
///   from the Rust type would be a lie. Supply `#[schema(...)]` explicitly.
/// - `#[serde(untagged)]` enums. `anyOf` with no discriminator is ambiguous to
///   decode, and the tie-break is inexpressible. Use an internally or
///   adjacently tagged enum, which becomes a `discriminator`.
/// - `#[serde(flatten)]` onto a map-typed field, which forces
///   `additionalProperties: true` on the parent.
/// - `#[serde(default)]` or `skip_serializing_if` on a non-`Option` field,
///   which would make `required` a lie.
/// - A `#[serde(other)]` catch-all variant under `openapi31` alone, which needs
///   3.2's `defaultMapping` to describe.
#[proc_macro_derive(Schema, attributes(schema))]
pub fn derive_schema(item: TokenStream) -> TokenStream {
    let _ = item;
    todo!()
}

/// Maps an error type to RFC 9457 problem details.
///
/// ```ignore
/// #[derive(ApiError)]
/// #[problem(base = "https://errors.example.com/")]
/// enum ApiError {
///     #[problem(status = 404, title = "User not found")]
///     NotFound { id: UserId },
///     #[problem(status = 409)]
///     EmailTaken,
/// }
/// ```
///
/// Also emits the `Responses` implementation, so the statuses the error can
/// produce and the statuses the description advertises cannot diverge.
#[proc_macro_derive(ApiError, attributes(problem))]
pub fn derive_api_error(item: TokenStream) -> TokenStream {
    let _ = item;
    todo!()
}

/// Declares a closed set of responses, one variant per status.
///
/// For an operation with more than one success shape. Modelled on
/// poem-openapi's `ApiResponse`, which is the best existing treatment of this.
#[proc_macro_derive(Reply, attributes(reply))]
pub fn derive_reply(item: TokenStream) -> TokenStream {
    let _ = item;
    todo!()
}

/// Declares a group of path parameters.
///
/// Field names must match the route template's variables; the route attribute
/// emits a const assertion comparing the two sets.
#[proc_macro_derive(PathParams, attributes(param))]
pub fn derive_path_params(item: TokenStream) -> TokenStream {
    let _ = item;
    todo!()
}

/// Declares a group of query parameters.
///
/// Rejects a nested object: `deepObject` is defined only for objects whose
/// properties are scalars, so anything deeper has no legal serialization. The
/// diagnostic points at `QueryString<T, M>`, which describes such a shape
/// properly under `openapi32`.
#[proc_macro_derive(QueryParams, attributes(param))]
pub fn derive_query_params(item: TokenStream) -> TokenStream {
    let _ = item;
    todo!()
}

/// Declares a group of request headers.
///
/// Rejects `Accept`, `Content-Type` and `Authorization`. The specification says
/// a parameter definition for those is ignored, so declaring one puts a claim
/// in the description that no consumer will honour. The diagnostic names the
/// right tool for each.
#[proc_macro_derive(Headers, attributes(header))]
pub fn derive_headers(item: TokenStream) -> TokenStream {
    let _ = item;
    todo!()
}

/// Declares a group of request cookies.
#[proc_macro_derive(Cookies, attributes(cookie))]
pub fn derive_cookies(item: TokenStream) -> TokenStream {
    let _ = item;
    todo!()
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
    let _ = item;
    todo!()
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
    let _ = item;
    todo!()
}

/// Declares an application context, emitting one `Provides` implementation per
/// field.
///
/// ```ignore
/// #[derive(Provider)]
/// struct App {
///     db: Pool,
///     #[provide(scoped)]
///     transaction: Transaction,
/// }
/// ```
///
/// A handler asking for something no field supplies fails to typecheck, rather
/// than panicking at runtime the way an erased state map does.
#[proc_macro_derive(Provider, attributes(provide))]
pub fn derive_provider(item: TokenStream) -> TokenStream {
    let _ = item;
    todo!()
}
