//! The ledger: one case per diagnostic the derives raise.
//!
//! Each derive is an attribute grammar over a closed key set, so what it owes
//! is a case per rule rather than a generator that would re-derive its match
//! arms — see `docs/testing.md`. The rows here assert *which* diagnostic fired;
//! the wording is held by the snapshots in `crates/kynos/tests/ui/macros/`,
//! where a reader sees it rendered.
//!
//! One ledger rather than one test module per derive, because the counters are
//! the point and they are the same counter six times over. A rule added to any
//! grammar without a row fails the build.

use proc_macro2::TokenStream as TokenStream2;
use syn::DeriveInput;

/// What was written, and a fragment of what the derive must say about it.
struct Case {
    description: &'static str,
    input: DeriveInput,
    expects: &'static str,
}

fn case(description: &'static str, declaration: TokenStream2, expects: &'static str) -> Case {
    Case {
        description,
        input: syn::parse2(declaration).expect("the case itself must parse"),
        expects,
    }
}

/// Runs a ledger against the expansion that owns it.
fn each_case_is_refused(ledger: Vec<Case>, expand: fn(&DeriveInput) -> syn::Result<TokenStream2>) {
    for Case {
        description,
        input,
        expects,
    } in ledger
    {
        let Err(error) = expand(&input) else {
            panic!("{description} must be rejected");
        };
        let reported = error.to_string();
        assert!(
            reported.contains(expects),
            "{description}: expected a diagnostic containing {expects:?}, got {reported:?}"
        );
    }
}

/// Counts a ledger against the diagnostic sites of the file it covers.
///
/// A count, not a mapping: it catches the drift that happens — a rule added
/// without a case — and not a row rewritten to reach a site another covers.
fn every_diagnostic_has_a_case(file: &str, source: &str, cases: usize) {
    let sites = source.matches("syn::Error::new(").count() + source.matches("meta.error(").count();
    assert_eq!(
        cases, sites,
        "`{file}` raises {sites} diagnostic(s) and {cases} have a case; a grammar rule added \
         without one is a rule that can stop firing silently"
    );
}

mod schema {
    use super::{Case, case, each_case_is_refused, every_diagnostic_has_a_case};
    use crate::derive::schema::expand_inner;

    fn ledger() -> Vec<Case> {
        vec![
            case(
                "a union, which no JSON value corresponds to",
                quote::quote!(
                    union Payload {
                        a: u32,
                    }
                ),
                "cannot describe a union",
            ),
            case(
                "`format`, which states what a value is rather than constraining it",
                quote::quote!(
                    struct Order {
                        #[schema(format = "uuid")]
                        id: String,
                    }
                ),
                "`format` says what a value",
            ),
            case(
                "`unique_items` given a value, when it is a flag",
                quote::quote!(
                    struct Order {
                        #[schema(unique_items = 1)]
                        tags: Vec<String>,
                    }
                ),
                "is a flag",
            ),
            case(
                "a numeric constraint given a string",
                quote::quote!(
                    struct Order {
                        #[schema(minimum = "x")]
                        total: u32,
                    }
                ),
                "takes a number",
            ),
            case(
                "a count constraint given a string",
                quote::quote!(
                    struct Order {
                        #[schema(min_length = "x")]
                        name: String,
                    }
                ),
                "takes a non-negative whole number",
            ),
            case(
                "a key outside the constraint grammar",
                quote::quote!(
                    struct Order {
                        #[schema(nonsense = 1)]
                        total: u32,
                    }
                ),
                "is not part of the `#[schema(...)]` grammar",
            ),
            case(
                "an untagged enum, which has no describable decoding rule",
                quote::quote!(
                    #[serde(untagged)]
                    enum Payload {
                        Number(u32),
                        Text(String),
                    }
                ),
                "an untagged enum",
            ),
        ]
    }

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
    }

    #[test]
    fn every_schema_diagnostic_has_a_case() {
        every_diagnostic_has_a_case("schema.rs", include_str!("schema.rs"), ledger().len());
    }

    /// `#[serde(untagged)]` on a struct is serde's diagnostic to raise, not ours.
    ///
    /// The refusal exists because an untagged *enum* has no describable
    /// decoding rule. A struct has no variants to choose between, so the
    /// sentence does not apply to one -- and serde already refuses the
    /// attribute there, in its own words. Raising a second diagnostic that
    /// calls a struct an enum is this derive restating a serde shape rule and
    /// getting the noun wrong, which is exactly what `Container` reads serde's
    /// attributes rather than re-deriving them in order to avoid.
    #[test]
    fn untagged_on_a_struct_is_left_to_serde() {
        let input: syn::DeriveInput = syn::parse2(quote::quote!(
            #[serde(untagged)]
            struct Receipt {
                total: u32,
            }
        ))
        .expect("the case itself must parse");

        let Err(error) = expand_inner(&input) else {
            return;
        };

        assert!(
            !error.to_string().contains("untagged enum"),
            "a struct was refused with a sentence about enums: {error}"
        );
    }
}

mod api_error {
    use super::{Case, case, each_case_is_refused, every_diagnostic_has_a_case};
    use crate::derive::api_error::expand_inner;

    fn ledger() -> Vec<Case> {
        vec![
            case(
                "a union",
                quote::quote!(
                    union StoreError {
                        a: u32,
                    }
                ),
                "cannot describe a union",
            ),
            case(
                "a status on the enum, where variants answer with their own",
                quote::quote!(
                    #[problem(status = 404)]
                    enum StoreError {
                        #[problem(status = 404)]
                        NotFound,
                    }
                ),
                "a status belongs on each variant",
            ),
            case(
                "a variant that never says what status it produces",
                quote::quote!(
                    enum StoreError {
                        NotFound,
                    }
                ),
                "does not say what status it produces",
            ),
            case(
                "a struct that never says what status it produces",
                quote::quote!(
                    struct StoreError;
                ),
                "does not say what status it produces",
            ),
            case(
                "a status outside the range a problem detail may carry",
                quote::quote!(
                    #[problem(status = 200)]
                    struct StoreError;
                ),
                "its status is between",
            ),
            case(
                "two statuses on one error, when a response has one",
                quote::quote!(
                    #[problem(status = 404, status = 410)]
                    struct StoreError;
                ),
                "already declares a status",
            ),
            case(
                "`base` on a variant, when it is the prefix the type shares",
                quote::quote!(
                    enum StoreError {
                        #[problem(status = 404, base = "https://example.com/")]
                        NotFound,
                    }
                ),
                "belongs on the type",
            ),
            case(
                "`extension` on the type, when it marks a field",
                quote::quote!(
                    #[problem(status = 404, extension)]
                    struct StoreError;
                ),
                "belongs on a field",
            ),
            case(
                "a key outside the problem grammar",
                quote::quote!(
                    #[problem(status = 404, titel = "User not found")]
                    struct StoreError;
                ),
                "is not part of the `#[problem(...)]` grammar",
            ),
            case(
                "an extension on a field with no name to publish it under",
                quote::quote!(
                    enum StoreError {
                        #[problem(status = 404)]
                        NotFound(#[problem(extension)] u64),
                    }
                ),
                "published under its field's name",
            ),
        ]
    }

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
    }

    #[test]
    fn every_api_error_diagnostic_has_a_case() {
        every_diagnostic_has_a_case("api_error.rs", include_str!("api_error.rs"), ledger().len());
    }
}

mod reply {
    use super::{Case, case, each_case_is_refused, every_diagnostic_has_a_case};
    use crate::derive::reply::expand_inner;

    fn ledger() -> Vec<Case> {
        vec![
            case(
                "a struct, when a reply is a closed set of responses",
                quote::quote!(
                    struct CreateReply;
                ),
                "closed set of responses",
            ),
            case(
                "a union",
                quote::quote!(
                    union CreateReply {
                        a: u32,
                    }
                ),
                "needs an enum",
            ),
            case(
                "a status on the enum, where variants answer with their own",
                quote::quote!(
                    #[reply(status = 200)]
                    enum CreateReply {
                        #[reply(status = 200)]
                        Ok(u32),
                    }
                ),
                "a status belongs on each variant",
            ),
            case(
                "a variant that never says what status it produces",
                quote::quote!(
                    enum CreateReply {
                        Ok(u32),
                    }
                ),
                "does not say what status it produces",
            ),
            case(
                "two variants answering with one status",
                quote::quote!(
                    enum UploadReply {
                        #[reply(status = 202)]
                        Queued(u32),
                        #[reply(status = 202)]
                        AlreadyQueued(u32),
                    }
                ),
                "already answers with",
            ),
            case(
                "a status outside the range a handler may answer with",
                quote::quote!(
                    enum CreateReply {
                        #[reply(status = 99)]
                        Ok(u32),
                    }
                ),
                "its status is between",
            ),
            case(
                "two statuses on one variant",
                quote::quote!(
                    enum CreateReply {
                        #[reply(status = 200, status = 201)]
                        Ok(u32),
                    }
                ),
                "already declares a status",
            ),
            case(
                "a key outside the reply grammar",
                quote::quote!(
                    enum CreateReply {
                        #[reply(status = 200, nonsense = "x")]
                        Ok(u32),
                    }
                ),
                "is not part of the `#[reply(...)]` grammar",
            ),
            case(
                "a struct variant, when a body is one described type",
                quote::quote!(
                    enum CreateReply {
                        #[reply(status = 201)]
                        Created { id: u32, revision: u32 },
                    }
                ),
                "carries its response body",
            ),
        ]
    }

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
    }

    #[test]
    fn every_reply_diagnostic_has_a_case() {
        every_diagnostic_has_a_case("reply.rs", include_str!("reply.rs"), ledger().len());
    }
}

mod security_scheme {
    use super::{Case, case, each_case_is_refused, every_diagnostic_has_a_case};
    use crate::derive::security_scheme::expand_inner;

    fn ledger() -> Vec<Case> {
        vec![
            case(
                "a scheme that never says what kind it is",
                quote::quote!(
                    struct Bearer;
                ),
                "must say what kind it is",
            ),
            case(
                "two kinds, when a scheme has exactly one",
                quote::quote!(
                    #[security(bearer, basic)]
                    struct Bearer;
                ),
                "exactly one kind",
            ),
            case(
                "a key outside the security grammar",
                quote::quote!(
                    #[security(nonsense)]
                    struct Bearer;
                ),
                "is not part of the `#[security(...)]` grammar",
            ),
            case(
                "an API key that never says where it travels",
                quote::quote!(
                    #[security(api_key(name = "X-Api-Key"))]
                    struct ApiKey;
                ),
                "must say where it travels",
            ),
            case(
                "an API key travelling somewhere it cannot",
                quote::quote!(
                    #[security(api_key(in = "path", name = "key"))]
                    struct ApiKey;
                ),
                "not `path`",
            ),
            case(
                "an API key that never says which field carries it",
                quote::quote!(
                    #[security(api_key(in = "header"))]
                    struct ApiKey;
                ),
                "must say which field carries it",
            ),
            case(
                "an API key claiming a header the specification reserves",
                quote::quote!(
                    #[security(api_key(in = "header", name = "authorization"))]
                    struct ApiKey;
                ),
                "must not be declared as a parameter",
            ),
        ]
    }

    /// The refusals the `oauth2` flow grammar adds.
    ///
    /// A second function rather than more rows in the first: the two are read
    /// together everywhere below, and one list of every diagnostic this derive
    /// raises had outgrown what a reader can hold — and what Clippy will accept.
    fn oauth2_ledger() -> Vec<Case> {
        vec![
            case(
                "an OAuth 2.0 scheme declaring no flow at all",
                quote::quote!(
                    #[security(oauth2(metadata_url = "https://auth.example.com/meta"))]
                    struct Delegated;
                ),
                "must declare at least one flow",
            ),
            case(
                "a flow OAuth 2.0 does not define",
                quote::quote!(
                    #[security(oauth2(magic_link(token_url = "https://auth.example.com/token")))]
                    struct Delegated;
                ),
                "is not an OAuth 2.0 flow",
            ),
            case(
                "a flow missing a URL its own grant needs",
                quote::quote!(
                    #[security(oauth2(authorization_code(
                        token_url = "https://auth.example.com/token"
                    )))]
                    struct Delegated;
                ),
                "authorization_url",
            ),
            case(
                "a carrier setting that is not the one word it takes",
                quote::quote!(
                    #[security(bearer)]
                    #[security(carrier = automatic)]
                    struct Bearer;
                ),
                "takes only `manual`",
            ),
            case(
                "one flow declared twice",
                quote::quote!(
                    #[security(oauth2(
                        client_credentials(token_url = "https://auth.example.com/a"),
                        client_credentials(token_url = "https://auth.example.com/b"),
                    ))]
                    struct Delegated;
                ),
                "already declared",
            ),
        ]
    }

    /// The two diagnostics that fire only where the document model has no field
    /// to hold the answer.
    ///
    /// Under `openapi32` both constructs are legal, so neither can be provoked
    /// and neither has a row. The count below adds them back, which is what
    /// keeps the ledger honest in both builds rather than in the one that
    /// happens to run first.
    #[cfg(not(feature = "openapi32"))]
    fn version_gated_ledger() -> Vec<Case> {
        vec![
            case(
                "a device authorization flow, which only 3.2 defines",
                quote::quote!(
                    #[security(oauth2(device_authorization(
                        device_authorization_url = "https://auth.example.com/device",
                        token_url = "https://auth.example.com/token"
                    )))]
                    struct Delegated;
                ),
                "openapi32",
            ),
            case(
                "an authorization server metadata URL, which only 3.2 carries",
                quote::quote!(
                    #[security(oauth2(
                        client_credentials(token_url = "https://auth.example.com/token"),
                        metadata_url = "https://auth.example.com/meta",
                    ))]
                    struct Delegated;
                ),
                "openapi32",
            ),
            case(
                "a deprecation, which only 3.2 has a field for",
                quote::quote!(
                    #[security(http(scheme = "bearer"), deprecated)]
                    struct Legacy;
                ),
                "openapi32",
            ),
        ]
    }

    #[cfg(feature = "openapi32")]
    fn version_gated_ledger() -> Vec<Case> {
        Vec::new()
    }

    /// How many diagnostics this build cannot provoke.
    ///
    /// Three, under `openapi32`: the constructs they refuse are legal there.
    const UNREACHABLE_HERE: usize = if cfg!(feature = "openapi32") { 3 } else { 0 };

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
        each_case_is_refused(oauth2_ledger(), expand_inner);
        each_case_is_refused(version_gated_ledger(), expand_inner);
    }

    #[test]
    fn every_security_scheme_diagnostic_has_a_case() {
        every_diagnostic_has_a_case(
            "security_scheme.rs",
            include_str!("security_scheme.rs"),
            ledger().len()
                + oauth2_ledger().len()
                + version_gated_ledger().len()
                + UNREACHABLE_HERE,
        );
    }

    /// A declared flow reaches the expansion.
    ///
    /// The defect this closes: `of_kind` built `OAuthFlows::default()`
    /// unconditionally and `check_kind` sent every flow to `skip_value`, so
    /// `#[security(oauth2(authorization_code(..)))]` described a scheme with no
    /// flows at all — and `examples/security_schemes.rs` shipped exactly that,
    /// emitting `{"type":"oauth2","flows":{}}` while presenting itself as the
    /// demonstration of delegated authorization.
    ///
    /// `kynos-macros` cannot depend on `kynos`, so the assertion is on the
    /// tokens rather than on the description they build;
    /// `crates/kynos/tests/derives.rs` is where the expansion is compiled.
    #[test]
    fn a_declared_flow_reaches_the_expansion() {
        let input: syn::DeriveInput = syn::parse_quote!(
            #[security(oauth2(authorization_code(
                authorization_url = "https://auth.example.com/authorize",
                token_url = "https://auth.example.com/token",
                refresh_url = "https://auth.example.com/token",
                scopes("users:read", "users:write"),
            )))]
            struct Delegated;
        );

        let expanded = expand_inner(&input)
            .expect("a well-formed oauth2 scheme")
            .to_string();

        for expected in [
            "with_authorization_code",
            "https://auth.example.com/authorize",
            "https://auth.example.com/token",
            "users:read",
            "users:write",
        ] {
            assert!(
                expanded.contains(expected),
                "the expansion never mentions {expected:?}: {expanded}"
            );
        }
    }
}

mod provider {
    use super::{Case, case, each_case_is_refused, every_diagnostic_has_a_case};
    use crate::derive::provider::expand_inner;

    fn ledger() -> Vec<Case> {
        vec![
            case(
                "two fields of one type, which a handler could not tell apart",
                quote::quote!(
                    struct App {
                        primary: Pool,
                        replica: Pool,
                    }
                ),
                "are both",
            ),
            // Two fields, because a lone type-parameter field has no sibling
            // implementation to overlap and is left to coherence.
            case(
                "a field typed by one of the context's own type parameters",
                quote::quote!(
                    struct App<T> {
                        pool: Pool,
                        value: T,
                    }
                ),
                "own type parameters",
            ),
        ]
    }

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
    }

    #[test]
    fn every_provider_diagnostic_has_a_case() {
        every_diagnostic_has_a_case("provider.rs", include_str!("provider.rs"), ledger().len());
    }
}

mod headers {
    use super::{Case, case, each_case_is_refused, every_diagnostic_has_a_case};
    use crate::derive::headers::expand_inner;

    fn ledger() -> Vec<Case> {
        vec![case(
            "a header the framework already negotiates",
            quote::quote!(
                struct Negotiation {
                    accept: String,
                }
            ),
            "must not be declared as a header parameter",
        )]
    }

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
    }

    #[test]
    fn every_headers_diagnostic_has_a_case() {
        every_diagnostic_has_a_case("headers.rs", include_str!("headers.rs"), ledger().len());
    }
}

mod tag {
    use super::{Case, each_case_is_refused, every_diagnostic_has_a_case};
    use crate::derive::tag::expand_inner;

    /// The 3.2-only members, which a 3.1 build refuses.
    ///
    /// Empty under `openapi32`, where all three are legal — the same shape
    /// [`super::security_scheme`] uses, and for the same reason: a diagnostic
    /// that only one build can provoke still has to be counted in both.
    #[cfg(not(feature = "openapi32"))]
    fn version_gated_ledger() -> Vec<Case> {
        use super::case;

        vec![
            case(
                "a summary, which only 3.2 gives a tag",
                quote::quote!(
                    #[tag(summary = "Everything about orders")]
                    struct Orders;
                ),
                "openapi32",
            ),
            case(
                "a kind, which only 3.2 gives a tag",
                quote::quote!(
                    #[tag(kind = "nav")]
                    struct Orders;
                ),
                "openapi32",
            ),
            case(
                "a parent, which only 3.2 gives a tag",
                quote::quote!(
                    #[tag(parent = Catalogue)]
                    struct Orders;
                ),
                "openapi32",
            ),
        ]
    }

    #[cfg(feature = "openapi32")]
    fn version_gated_ledger() -> Vec<Case> {
        Vec::new()
    }

    /// How many diagnostics this build cannot provoke.
    ///
    /// One, under `openapi32`: the three members share a single site, and what
    /// it refuses is legal there.
    const UNREACHABLE_HERE: usize = if cfg!(feature = "openapi32") { 1 } else { 0 };

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(version_gated_ledger(), expand_inner);
    }

    #[test]
    fn every_tag_diagnostic_has_a_case() {
        // The three members are refused from one `syn::Error::new`, so the
        // ledger's three cases meet one site. Counting the *site* is the point:
        // a fourth 3.2 member added without a case fails here.
        let covered = usize::from(!version_gated_ledger().is_empty()) + UNREACHABLE_HERE;
        every_diagnostic_has_a_case("tag.rs", include_str!("tag.rs"), covered);
    }
}
