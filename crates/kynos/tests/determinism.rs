//! That a description is the same bytes in every process that emits it.
//!
//! `nfr.md` states the guarantee — *"emitted documents are byte-deterministic
//! across runs and platforms"* — and Beam's acceptance contract gates its
//! migration on it, because a fixture corpus a downstream generator is tested
//! against is worthless if regenerating it produces a different file.
//!
//! Ordering is `IndexMap`-backed throughout the model, so determinism is a
//! design property rather than a sorting pass. What is unproven is that nothing
//! *upstream* of the model reaches an unordered collection on the way in: the
//! registry, the router and the validator each keep a `HashMap`, and any one of
//! them iterated rather than indexed would randomize the output.
//!
//! # Why a second process rather than a second call
//!
//! `properties.rs` already asserts that emitting one document twice gives one
//! string, and that is the weaker half. `RandomState` is seeded once per
//! process, so two `HashMap`s holding the same keys iterate the same way for
//! the life of that process — a same-process comparison agrees with itself
//! whether or not a `HashMap` reached the output. Only a fresh process draws a
//! fresh seed, which is why this test spawns one.

#![cfg(all(feature = "macros", feature = "json"))]

use std::process::Command;

use kynos::prelude::*;
use serde::{Deserialize, Serialize};

/// Delimits the payload from the harness's own chatter on the child's stdout.
const BEGIN: &str = "-----BEGIN KYNOS DOCUMENT-----";
/// The closing delimiter.
const END: &str = "-----END KYNOS DOCUMENT-----";

/// How many independent processes the document is emitted in.
///
/// Two would catch a randomized order almost always; a third costs one process
/// and removes the "almost".
const PROCESSES: usize = 3;

// The fixture is deliberately wider than any one assertion needs. Determinism
// is a property of the whole emission path, so the document has to reach every
// collection that path touches: nested and recursive components, several tags,
// a security scheme, path and query parameters, and more than one operation per
// path item.

/// A postal address, reached only through `Customer`.
#[derive(Schema, Serialize, Deserialize)]
struct Address {
    line: String,
    city: String,
}

/// A node that refers to itself, so the registry reserves before it registers.
#[derive(Schema, Serialize, Deserialize)]
struct Category {
    name: String,
    parent: Option<Box<Category>>,
}

/// How an order was paid for.
#[derive(Schema, Serialize, Deserialize)]
#[serde(tag = "kind")]
enum Payment {
    /// Settled against a card.
    Card { last4: String },
    /// Settled from a balance.
    Balance,
}

/// Someone who places orders.
#[derive(Schema, Serialize, Deserialize)]
struct Customer {
    id: u64,
    name: String,
    address: Address,
    category: Category,
}

/// One order.
#[derive(Schema, Serialize, Deserialize)]
struct Order {
    id: u64,
    customer: Customer,
    payment: Payment,
}

/// What `/customers/{id}` captures.
#[allow(dead_code)]
#[derive(Schema, PathParams)]
struct CustomerPath {
    id: u64,
}

/// How a listing is narrowed.
#[allow(dead_code)]
#[derive(Schema, QueryParams)]
struct Page {
    limit: Option<u32>,
    cursor: Option<String>,
}

/// The key callers present.
#[derive(SecurityScheme)]
#[security(api_key(in = "header", name = "X-Api-Key"))]
struct ApiKey;

/// Lists customers.
#[kynos::get("/customers")]
async fn list_customers(Query(_page): Query<Page>) -> Json<Vec<Customer>> {
    Json(Vec::new())
}

/// Fetches one customer.
#[kynos::get("/customers/{id}")]
async fn get_customer(Path(_path): Path<CustomerPath>) -> Json<Customer> {
    unreachable!("this fixture is described, never served")
}

/// Replaces one customer.
#[kynos::put("/customers/{id}")]
async fn put_customer(
    Path(_path): Path<CustomerPath>,
    Json(body): Json<Customer>,
) -> Json<Customer> {
    Json(body)
}

/// Places an order.
#[kynos::post("/orders")]
async fn create_order(Json(order): Json<Order>) -> Created<Json<Order>> {
    Created::at("/orders/1", Json(order))
}

/// The fixture description, as pretty-printed JSON.
fn document() -> String {
    Router::<()>::new()
        .security_scheme::<ApiKey>()
        .tag::<Customers>()
        .tag::<Orders>()
        .mount(kynos::routes![
            list_customers,
            get_customer,
            put_customer,
            create_order
        ])
        .openapi()
        .expect("the fixture describes itself")
        .to_json()
        .expect("a description serializes")
}

/// Everything about customers.
#[derive(Tag)]
struct Customers;

/// Everything about orders.
#[derive(Tag)]
struct Orders;

/// Emits the fixture description between markers.
///
/// This is the child half of
/// [`a_description_is_the_same_bytes_in_every_process`]. It is a `#[test]`
/// rather than a binary because an integration target has no `main` to reach,
/// and running it directly costs one emission and asserts the fixture still
/// describes itself.
#[test]
fn emit_fixture_description() {
    println!("{BEGIN}");
    println!("{}", document());
    println!("{END}");
}

/// Runs this binary again and returns what its child emitted.
fn emit_in_a_fresh_process() -> String {
    let executable = std::env::current_exe().expect("a test binary knows its own path");

    let output = Command::new(&executable)
        .args([
            "--exact",
            "emit_fixture_description",
            "--nocapture",
            "--test-threads=1",
        ])
        .output()
        .expect("the test binary re-executes");

    assert!(
        output.status.success(),
        "the child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("the child emits UTF-8");
    let (_, after) = stdout
        .split_once(BEGIN)
        .expect("the child emitted an opening marker");
    let (payload, _) = after
        .split_once(END)
        .expect("the child emitted a closing marker");

    payload.trim().to_owned()
}

/// The whole guarantee: one API, one document, whatever process emits it.
///
/// A failure here means something on the emission path iterated a collection
/// that has no order — the registry's `origins`, the router's `index_of`, or
/// one of the validator's sets. Each is indexed rather than walked today, and
/// this is what keeps it that way.
#[test]
fn a_description_is_the_same_bytes_in_every_process() {
    let mut emissions = Vec::with_capacity(PROCESSES);
    for _ in 0..PROCESSES {
        emissions.push(emit_in_a_fresh_process());
    }

    assert!(
        !emissions[0].is_empty(),
        "the fixture emitted an empty document"
    );

    for (index, emission) in emissions.iter().enumerate().skip(1) {
        if let Some(report) = first_difference(&emissions[0], emission) {
            panic!("process 0 and process {index} emitted different bytes\n{report}");
        }
    }
}

/// Where two emissions part company, as a line number and the two lines.
///
/// The whole document is several hundred lines, so `assert_eq!` on it reports a
/// difference by printing both copies and leaving the reader to find it. This
/// names the line instead.
fn first_difference(left: &str, right: &str) -> Option<String> {
    for (number, (one, other)) in left.lines().zip(right.lines()).enumerate() {
        if one != other {
            return Some(format!(
                "first differing line is {}:\n  process 0: {one}\n  the other: {other}",
                number + 1
            ));
        }
    }

    (left.lines().count() != right.lines().count()).then(|| {
        format!(
            "they agree line for line but differ in length: {} lines against {}",
            left.lines().count(),
            right.lines().count()
        )
    })
}

/// The order `components/schemas` carries, stated rather than merely observed.
///
/// `docs/schema.md` records this as the ordering contract, and it follows from
/// `Registry::resolve` registering a type *after* descending into its fields: a
/// component a field refers to is inserted before the component that refers to
/// it. The document is otherwise free to grow, so this asserts the relative
/// order of a known chain rather than the whole list.
#[test]
fn a_component_is_registered_after_everything_it_refers_to() {
    let document = document();
    let names: Vec<&str> = ["Address", "Category", "Customer", "Payment", "Order"]
        .into_iter()
        .filter(|name| document.contains(&format!("\"{name}\"")))
        .collect();

    assert_eq!(
        names.len(),
        5,
        "the fixture no longer registers every component this asserts over"
    );

    let position = |name: &str| {
        document
            .find(&format!("\"{name}\": {{"))
            .unwrap_or_else(|| panic!("`{name}` is a declared component"))
    };

    assert!(
        position("Address") < position("Customer"),
        "a field's component is registered before the type carrying it"
    );
    assert!(
        position("Category") < position("Customer"),
        "a recursive component is registered before the type carrying it"
    );
    assert!(
        position("Customer") < position("Order"),
        "the whole chain is registered depth-first"
    );
}
