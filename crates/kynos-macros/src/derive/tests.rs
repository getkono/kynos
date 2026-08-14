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

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
    }

    #[test]
    fn every_security_scheme_diagnostic_has_a_case() {
        every_diagnostic_has_a_case(
            "security_scheme.rs",
            include_str!("security_scheme.rs"),
            ledger().len(),
        );
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
