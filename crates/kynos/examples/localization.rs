//! Serving one resource in three languages.
//!
//! ```text
//! cargo run -p kynos --example localization
//! ```
//!
//! ```text
//! curl -i localhost:3000/greeting -H 'Accept-Language: fr-CA, en;q=0.5'
//! curl -i localhost:3000/greeting -H 'Accept-Language: ja'
//! ```
//!
//! Four things are worth noticing, and the first is the one that surprises
//! people coming from `Accept`:
//!
//! * **`Accept-Language` *is* a parameter.** OpenAPI names exactly three header
//!   fields whose parameter definition shall be ignored — `Accept`,
//!   `Content-Type` and `Authorization` — and this is not one of them. So
//!   `negotiation.rs` is right that declaring `Accept` is a claim no consumer
//!   will honour, and this file is right to declare the opposite. The two axes
//!   look alike and their descriptions are shaped differently for a reason the
//!   specification gives.
//!
//! * **The offer is stated on the response, not the request.** The parameter's
//!   schema is an unconstrained string, because the value is a *priority list*:
//!   `da, en-gb;q=0.8, en;q=0.7` is not a member of any set of tags. The offer
//!   is enumerated on `Content-Language`, where it is true — that field carries
//!   one tag, and `Localized` has no public constructor, so the only value that
//!   can reach the wire is one the negotiation chose.
//!
//! * **Nothing here can fail, and the description says so.** There is no 406
//!   and no 400: a client asking for a language nobody offers is served the
//!   default, and a range the parser cannot read is dropped while the rest of
//!   the field still counts. Adding language negotiation to an operation adds
//!   no status to it. What makes that honest rather than silent is
//!   `Content-Language`, which is `required` — a client that cannot use the
//!   fallback can always see that it got one.
//!
//! * **The catalogue is yours.** Kynos negotiates the language and states which
//!   one it chose; the strings are the application's, and the `HashMap` below
//!   could as easily be Fluent, gettext, or files on disk. Kynos ships no
//!   translations and no message-format model — see `docs/architecture.md`'s
//!   third invariant for why that is a refusal rather than an omission.
//!
//! The offer is written down rather than discovered, because
//! `Describe::describe` takes no `self`: a set held in a value cannot reach the
//! description at all.

use std::{collections::HashMap, net::Ipv4Addr};

use kynos::{
    Router,
    extract::body::text::Text,
    response::language::{AcceptLanguage, Localized, offer::Languages},
    server::Server,
};

/// The languages this service answers in.
///
/// The first is the default: it is what a request carrying no preference gets,
/// and what one whose preferences match nothing gets. Every tag here is checked
/// for RFC 5646 well-formedness while the program compiles, so `en_GB` — the
/// POSIX spelling, with an underscore — would not build.
struct Spoken;

impl Languages for Spoken {
    const TAGS: &'static [&'static str] = &["en", "fr", "de"];
}

/// The greetings, keyed by the tags the offer names.
///
/// An ordinary map built at startup. Nothing about it is Kynos's: what the
/// framework supplied is the `&'static str` used to index it.
fn catalogue() -> HashMap<&'static str, &'static str> {
    HashMap::from([("en", "Hello"), ("fr", "Bonjour"), ("de", "Guten Tag")])
}

/// A greeting in whichever language the client and the service agree on.
///
/// The closure receives the chosen tag, which is the shape difference from
/// `Accept::respond_with`: a handler needs the language *before* it builds
/// anything, because the language is what indexes the catalogue.
#[kynos::get("/greeting")]
async fn greeting(preferred: AcceptLanguage<Spoken>) -> Localized<Text, Spoken> {
    let greetings = catalogue();

    preferred.respond_with(|language| {
        Text(
            greetings
                .get(language)
                .copied()
                .unwrap_or("Hello")
                .to_owned(),
        )
    })
}

/// A program generic over what it offers.
///
/// `Languages` is public and unsealed, unlike `Representations`: the offerable
/// codecs are Kynos's to know, and a catalogue is not.
fn languages_offered<L: Languages>() -> &'static [&'static str] {
    L::TAGS
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    // Knowable without a request, which is what makes it describable at all.
    println!("answering in {:?}", languages_offered::<Spoken>());

    // Lookup falls back by truncating the range, so a client asking for
    // Canadian French is served the French this service does stock.
    println!(
        "`fr-CA` is served {}",
        AcceptLanguage::<Spoken>::parse("fr-CA").choose()
    );
    // And a language nobody offers takes the default rather than a 406.
    println!(
        "`ja` is served {}",
        AcceptLanguage::<Spoken>::parse("ja").choose()
    );

    let router = Router::<()>::new().mount(kynos::routes![greeting]);

    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
