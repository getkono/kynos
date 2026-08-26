//! Describing a type: constraints, serde interop, map keys, and the escape
//! hatch.
//!
//! ```text
//! cargo run -p kynos --example schema
//! ```
//!
//! Four things are worth noticing:
//!
//! * **The serde attributes are read, not repeated.** `rename_all` and
//!   `#[serde(tag = ...)]` are already on the type because it has to serialize;
//!   the schema derive reads the same declaration, so the wire form and the
//!   description come from one place and cannot drift. There is no parallel
//!   `#[schema(rename_all = ...)]` to forget.
//! * **`#[schema(...)]` carries constraints and nothing else.** Its keys are
//!   exactly the fields of [`Constraints`](kynos::schema::constraints), so the
//!   attribute and the type it fills cannot disagree. `format` is rejected: a
//!   constraint is a business rule about one field, and a format is a claim
//!   about what a type *is*. See [`scalars.rs`](scalars.rs) for the latter.
//! * **A map key is a trait, not an annotation.** JSON object keys are strings,
//!   so `MapKey` produces the key's *constraints* rather than its schema —
//!   string-ness is then true by construction rather than a promise nothing
//!   checks.
//! * **Weakness is allowed; silent weakness is not.** `Unchecked<T>` is the one
//!   way to put an unconstrained value in a body. It annotates the schema and
//!   makes `validate` warn, and `deny_unchecked_schemas` turns that warning
//!   into a build error for a team that wants none at all.

use std::{collections::HashMap, net::Ipv4Addr};

use kynos::{
    prelude::*,
    schema::{MapKey, constraints::Constraints, unchecked::Unchecked},
    server::Server,
};
use serde::{Deserialize, Serialize};

/// A stock-keeping unit, used as a map key.
///
/// A newtype rather than a `String` for the same reason a UUID is not one: the
/// shape is part of what the value is.
#[derive(Schema, Serialize, Deserialize, PartialEq, Eq, Hash)]
struct Sku(String);

/// `propertyNames` for a map keyed by this.
///
/// `Constraints` is `#[non_exhaustive]`, so it grows without breaking callers —
/// which is also why this starts from `default` rather than a literal.
impl MapKey for Sku {
    fn key_constraints() -> Constraints {
        let mut constraints = Constraints::default();
        constraints.pattern = Some("^[A-Z]{3}-[0-9]{4}$".to_owned());
        constraints
    }
}

/// A product in the catalogue.
///
/// `rename_all` is serde's, and the description follows it: the emitted
/// property names are `displayName` and `priceCents`, which is what a consumer
/// actually receives.
#[derive(Schema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Product {
    /// Constraints become JSON Schema assertions *and* the parser's checks, so
    /// a name of 200 characters is rejected before the handler runs and the
    /// document says why.
    #[schema(min_length = 1, max_length = 120)]
    display_name: String,

    #[schema(pattern = "^[a-z][a-z0-9-]*$")]
    slug: String,

    #[schema(minimum = 0, maximum = 1_000_000)]
    price_cents: u64,

    /// `unique_items` is a flag, because `uniqueItems: false` is the default
    /// and saying it changes nothing.
    #[schema(min_items = 1, max_items = 10, unique_items)]
    tags: Vec<String>,

    /// An `Option` is what makes a property optional. There is no
    /// `#[schema(required = false)]`, because `required` follows from the type.
    #[schema(max_length = 2_000)]
    summary: Option<String>,
}

/// How a product was priced.
///
/// An internally tagged enum becomes a `oneOf` with a `discriminator`, so a
/// consumer decodes by reading one property rather than by trying each branch.
/// `#[serde(untagged)]` is rejected for exactly that reason: `anyOf` with no
/// discriminator is ambiguous, and serde's first-match tie-break has no
/// expression in JSON Schema.
#[derive(Schema, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Pricing {
    /// One price, always.
    Fixed { cents: u64 },
    /// A price that depends on how many are bought.
    Tiered { tiers: Vec<Tier> },
}

/// One step of a tiered price.
#[derive(Schema, Serialize, Deserialize)]
struct Tier {
    #[schema(minimum = 1)]
    from_quantity: u32,
    #[schema(minimum = 0)]
    cents: u64,
}

/// Everything the catalogue knows, keyed by SKU.
///
/// The map's `propertyNames` carries `Sku`'s pattern. A map keyed by `String`
/// emits none, because a vacuous constraint is noise.
#[derive(Schema, Serialize, Deserialize)]
struct Catalogue {
    products: HashMap<Sku, Product>,
    pricing: HashMap<Sku, Pricing>,
}

/// A supplier's feed, whose shape this service does not control.
///
/// `Unchecked` is the honest way to say so. The alternative — a
/// `serde_json::Value` field — has no `Schema` implementation at all, so it
/// cannot reach a body and this is the only route.
#[derive(Schema, Serialize, Deserialize)]
struct Ingest {
    /// Constrained, because the parts this service does control are known.
    #[schema(pattern = "^[a-z0-9-]+$")]
    supplier: String,

    /// Not constrained, and the document says so rather than implying an
    /// object with no properties.
    payload: Unchecked<serde_json::Value>,
}

/// Returns the whole catalogue.
#[kynos::get("/catalogue")]
async fn get_catalogue() -> Json<Catalogue> {
    let sku = Sku("ABC-0001".to_owned());

    // Every value here satisfies the constraints declared above -- the SKU
    // matches `propertyNames`, the slug matches its pattern, the tag list is
    // within its bounds. A schema nothing in the file could satisfy would be a
    // schema worth doubting.
    let product = Product {
        display_name: "Mechanical Keyboard".to_owned(),
        slug: "mechanical-keyboard".to_owned(),
        price_cents: 12_950,
        tags: vec!["input".to_owned(), "keyboard".to_owned()],
        summary: Some("Eighty-seven keys and no regrets.".to_owned()),
    };

    Json(Catalogue {
        products: HashMap::from([(Sku(sku.0.clone()), product)]),
        pricing: HashMap::from([(
            sku,
            Pricing::Tiered {
                tiers: vec![
                    Tier {
                        from_quantity: 1,
                        cents: 12_950,
                    },
                    Tier {
                        from_quantity: 10,
                        cents: 11_500,
                    },
                ],
            },
        )]),
    })
}

/// Accepts a supplier feed.
#[kynos::post("/ingest")]
async fn post_ingest(Json(ingest): Json<Ingest>) -> NoContent {
    // The constrained half is a `String` and the unconstrained half is behind
    // `Unchecked`, so reading them looks different in exactly the way the
    // description says they are.
    println!("{}: {}", ingest.supplier, ingest.payload.into_inner());
    NoContent
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new().mount(kynos::routes![get_catalogue, post_ingest]);

    // `validate` reports the `Unchecked` field as a warning. Uncommenting
    // `deny_unchecked_schemas` above would make it a build error instead, and
    // `post_ingest` would have to describe its payload or go.
    for violation in router.validate()? {
        println!("{violation}");
    }

    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
