//! Localizing a problem detail from the application's own catalogue.
//!
//! ```text
//! cargo run -p kynos --example localized_errors
//! ```
//!
//! ```text
//! curl -i localhost:3000/users/9 -H 'Accept-Language: fr'
//! curl -i localhost:3000/users/9 -H 'Accept-Language: ja'
//! ```
//!
//! RFC 9457 says a problem's `title` "SHOULD NOT change from occurrence to
//! occurrence of the problem, **except for localization** (e.g., using
//! proactive content negotiation)". This is that, and everything it needs is
//! public API — there is no `Localize` trait in Kynos, deliberately.
//!
//! # Why an interceptor rather than a return type
//!
//! `IntoResponse::into_response(self)` takes no context, and a rejection
//! short-circuits straight to a response before a handler runs. So there is no
//! point in the pipeline where a request and a `Problem` coexist — which means
//! localizing an error is something that happens *to* a finished response, and
//! an interceptor is the only thing that sees one.
//!
//! The cost is a JSON round trip on the error path. That is the same shape
//! `Compression` already has, errors are not the hot path, and the alternative
//! is threading a locale through every response type in the framework.
//!
//! # What this reaches, and it is more than it looks
//!
//! Every problem the service can produce, because a rejection's response
//! travels back up through `next.run` like any other. The 404 below is the
//! application's own `#[derive(ApiError)]`; a malformed path parameter's 400 is
//! Kynos's, declared by an extractor this file never names, and it is localized
//! by the same six lines.
//!
//! # What Kynos will not do
//!
//! * **Ship translations of its own reason phrases.** Roughly forty phrases in
//!   however many languages, none of which CI could hold correct — a table only
//!   a native speaker can verify is not one this project can keep. A wrong
//!   translation misleads a human where an English one merely fails to help.
//! * **Localize `detail`.** It comes from `Display`, so translating it means
//!   owning argument reordering, plural categories and gendered agreement —
//!   a message-format model, which is `fluent` or `icu` and a dependency row
//!   `architecture.md` does not have. RFC 9457 section 3.1.4 points the other
//!   way anyway: consumers "SHOULD NOT parse the `detail` member", and the
//!   machine-readable channel is `type` plus the extension members.
//!
//! # Where it sits in a chain
//!
//! Inside `Compression`, which would otherwise encode bytes this then rewrites.
//! Inside `Cache` too, though that one is benign: only a 200 is stored, and a
//! localized problem is never one.
//!
//! It declares `Content-Language` and no status, so it composes with anything
//! that does not also set that field.

use std::{collections::HashMap, net::Ipv4Addr};

use http_body_util::BodyExt;
use kynos::{
    Router,
    extract::{body::json::Json, params::path::Path},
    http::{Request, header},
    middleware::{Continued, Interceptor, Next},
    response::language::{
        AcceptLanguage, headers::ContentLanguage, offer::Languages, tag::LanguageTag,
    },
    server::Server,
};
use serde::{Deserialize, Serialize};

/// The languages the catalogue answers in.
struct Spoken;

impl Languages for Spoken {
    const TAGS: &'static [&'static str] = &["en", "fr"];
}

/// The field the interceptor reads, declared so it is described.
///
/// `rename` because the derive takes a field's identifier verbatim, and a Rust
/// identifier cannot hold the hyphen an HTTP field name does.
#[derive(kynos::HeaderParams)]
struct Negotiation {
    /// The natural languages preferred in the response.
    #[header(rename = "accept-language")]
    accept_language: Option<String>,
}

/// Titles, keyed the way RFC 9457 keys one: by the problem's *type*.
///
/// The status is part of the key because a problem with no `#[problem(base)]`
/// is `about:blank`, whose title section 4.2.1 makes a function of the status
/// alone — so every rejection Kynos raises shares one URI and is told apart
/// only by its code.
fn catalogue() -> HashMap<(&'static str, u16, &'static str), &'static str> {
    HashMap::from([
        (
            ("https://errors.example.com/no-such-user", 404, "fr"),
            "Utilisateur introuvable",
        ),
        (("about:blank", 400, "fr"), "Requête incorrecte"),
        (("about:blank", 404, "fr"), "Introuvable"),
    ])
}

/// Replaces a problem's `title` with the catalogue's, in the chosen language.
struct Localize {
    titles: HashMap<(&'static str, u16, &'static str), &'static str>,
}

impl<C: Sync + 'static> Interceptor<C> for Localize {
    type Reads = Negotiation;
    type Adds = ContentLanguage;
    type Short = std::convert::Infallible;

    async fn intercept(
        &self,
        request: Request,
        reads: Negotiation,
        _context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<ContentLanguage>, Self::Short> {
        // The framework's matcher, run against this application's own offer.
        let field = reads.accept_language.unwrap_or_default();
        let language = AcceptLanguage::<Spoken>::parse(&field).choose();

        let mut continued = next.run(request).await;

        let is_problem = continued
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("application/problem+json"));

        if is_problem {
            let status = continued.status().as_u16();
            let body = continued.take_body();

            if let Ok(collected) = body.collect().await {
                let bytes = collected.to_bytes();
                continued.set_body(kynos::http::body::Body::from_bytes(
                    self.retitle(&bytes, status, language),
                ));
            }
        }

        // `choose` returns one of `Spoken::TAGS`, and every entry there was
        // checked for well-formedness while this program compiled -- so the
        // parse cannot fail, and the public constructor takes a parsed tag
        // precisely so nothing else can put junk in the field.
        let tag = LanguageTag::parse(language).expect("an offered tag is well-formed");

        Ok(continued.with_headers(ContentLanguage::new(&tag)))
    }
}

impl Localize {
    /// Rewrites `title` where the catalogue has one, and leaves the document
    /// untouched where it does not.
    ///
    /// An untranslated problem keeps its source-language title rather than
    /// losing one, which is what makes adding a language additive.
    fn retitle(&self, body: &[u8], status: u16, language: &'static str) -> bytes::Bytes {
        let Ok(mut problem) = serde_json::from_slice::<serde_json::Value>(body) else {
            return bytes::Bytes::copy_from_slice(body);
        };

        let type_uri = problem
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("about:blank")
            .to_owned();

        if let Some(title) = self.titles.get(&(type_uri.as_str(), status, language))
            && let Some(object) = problem.as_object_mut()
        {
            object.insert("title".to_owned(), serde_json::Value::from(*title));
        }

        serde_json::to_vec(&problem)
            .map_or_else(|_| bytes::Bytes::copy_from_slice(body), bytes::Bytes::from)
    }
}

#[derive(Serialize, Deserialize, kynos::Schema)]
struct User {
    id: u64,
    name: String,
}

#[derive(kynos::Schema, kynos::PathParams)]
struct UserPath {
    id: u64,
}

/// The application's own error, with a `base` so its type URI is not
/// `about:blank` — which is what lets a catalogue tell it from a rejection.
#[derive(Debug, thiserror::Error, kynos::ApiError)]
#[problem(base = "https://errors.example.com/")]
enum StoreError {
    #[error("no user with that id")]
    #[problem(status = 404, title = "No such user")]
    NoSuchUser,
}

#[kynos::get("/users/{id}")]
async fn get_user(Path(path): Path<UserPath>) -> Result<Json<User>, StoreError> {
    if path.id == 1 {
        Ok(Json(User {
            id: 1,
            name: "Ada".to_owned(),
        }))
    } else {
        Err(StoreError::NoSuchUser)
    }
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new()
        .mount(kynos::routes![get_user])
        .intercept(Localize {
            titles: catalogue(),
        });

    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
