//! That a field's `#[schema(...)]` constraints are what `Json` enforces, and
//! every other input that deserializes a `Schema` type with it.
//!
//! A constraint is one declaration with two projections: the keyword the
//! description emits, and the check the derive generates after
//! deserialization. Each document below is read both ways — by `Json<T>`, and
//! by a real JSON Schema validator over the schema `T` emits — and the two must
//! agree on whether it is admitted. Where the body is refused, the refusal is a
//! 422 keyed by the JSON Pointer of the member that broke its bound.
//!
//! `pattern` is the one constraint left out: it is emitted and not yet
//! enforced, which `kynos::schema::constraints` records.

// `test-util` carries the JSON Schema validator, which is what makes this a
// check against an oracle rather than an assertion written from the derive.
#![cfg(all(feature = "macros", feature = "json", feature = "test-util"))]

use std::collections::{BTreeMap, BTreeSet};

use kynos::{
    Schema,
    error::rejection::BodyRejection,
    extract::{FromRequest, body::json::Json},
    http::{HeaderValue, Request, body::Body, header},
    schema::Schema as SchemaTrait,
};
use serde::Deserialize;
use serde_json::{Value, json};

/// The schema `T` emits, with everything it refers to reachable from the root.
fn emitted<T: SchemaTrait>() -> Value {
    let mut registry = kynos::schema::registry::Registry::new();
    let body = T::schema(&mut registry);

    let mut root = serde_json::to_value(body).expect("a schema serializes");
    let components =
        serde_json::to_value(registry.into_components()).expect("the components serialize");
    root.as_object_mut()
        .expect("a derived schema is an object")
        .insert("components".to_owned(), components);
    root
}

/// What `Json<T>` makes of `document`: `None` where it is admitted, and the
/// failures keyed by pointer where it is refused as not fitting its schema.
async fn read<T>(document: &Value) -> Option<BTreeMap<String, String>>
where
    Json<T>: FromRequest<(), Rejection = BodyRejection>,
{
    let bytes = serde_json::to_vec(document).expect("a document serializes");
    let mut request = Request::new(Body::from_bytes(bytes::Bytes::from(bytes)));
    request.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );

    match Json::<T>::from_request(request, &()).await {
        Ok(_) => None,
        Err(BodyRejection::Schema { failures }) => Some(failures),
        Err(other) => panic!("a well-formed document refused as something else: {other:?}"),
    }
}

/// Holds `Json<T>` to the validator over `document`, and returns where it
/// refused the document, if it did.
async fn agree<T>(document: Value) -> Option<Vec<String>>
where
    T: SchemaTrait,
    Json<T>: FromRequest<(), Rejection = BodyRejection>,
{
    let schema = emitted::<T>();
    let validator =
        jsonschema::draft202012::new(&schema).expect("an emitted schema compiles as draft 2020-12");
    let admitted = validator.is_valid(&document);
    let refused = read::<T>(&document).await;

    assert_eq!(
        admitted,
        refused.is_none(),
        "the validator and `Json` disagree on {document}: refused at {refused:?}\nschema: {schema}"
    );
    refused.map(|failures| failures.into_keys().collect())
}

/// Asserts `document` is admitted.
async fn admits<T>(document: Value)
where
    T: SchemaTrait,
    Json<T>: FromRequest<(), Rejection = BodyRejection>,
{
    assert_eq!(agree::<T>(document).await, None);
}

/// Asserts `document` is refused at exactly `pointers`.
async fn refuses<T>(document: Value, pointers: &[&str])
where
    T: SchemaTrait,
    Json<T>: FromRequest<(), Rejection = BodyRejection>,
{
    let expected: Vec<String> = pointers.iter().map(|&pointer| pointer.to_owned()).collect();
    assert_eq!(agree::<T>(document).await, Some(expected));
}

/// A member that carries a bound of its own, reached through each container.
#[derive(Debug, Schema, Deserialize)]
struct Line {
    #[schema(minimum = 1)]
    quantity: u32,
}

/// Every enforced keyword, on every kind of value that takes one.
#[derive(Debug, Schema, Deserialize)]
struct Order {
    #[schema(min_length = 2, max_length = 4)]
    name: String,
    #[schema(minimum = 1, maximum = 10)]
    seats: u32,
    #[schema(exclusive_minimum = 0, exclusive_maximum = 1)]
    ratio: f64,
    #[schema(multiple_of = 5)]
    step: i64,
    #[schema(min_items = 1, max_items = 2, unique_items)]
    tags: Vec<String>,
    #[schema(max_length = 3)]
    #[serde(default)]
    nickname: Option<String>,
    first: Line,
    lines: Vec<Line>,
    #[serde(rename = "by/sku")]
    by_sku: BTreeMap<String, Line>,
}

fn order() -> Value {
    json!({
        "name": "abc",
        "seats": 3,
        "ratio": 0.5,
        "step": 10,
        "tags": ["a", "b"],
        "nickname": null,
        "first": { "quantity": 1 },
        "lines": [{ "quantity": 1 }, { "quantity": 2 }],
        "by/sku": { "x~y": { "quantity": 1 } },
    })
}

/// `order()` with `member` replaced by `value`.
fn order_with(member: &str, value: Value) -> Value {
    let mut document = order();
    document[member] = value;
    document
}

#[tokio::test]
async fn a_document_inside_every_bound_is_admitted() {
    admits::<Order>(order()).await;
    admits::<Order>(order_with("nickname", json!("abc"))).await;
    admits::<Order>(order_with("name", json!("ab"))).await;
    admits::<Order>(order_with("seats", json!(10))).await;
    admits::<Order>(order_with("seats", json!(1))).await;
    admits::<Order>(order_with("tags", json!(["a"]))).await;
}

#[tokio::test]
async fn a_string_is_bounded_in_characters_not_bytes() {
    // Four characters, twelve bytes in UTF-8.
    admits::<Order>(order_with("name", json!("ééé€"))).await;
    refuses::<Order>(order_with("name", json!("a")), &["/name"]).await;
    refuses::<Order>(order_with("name", json!("abcde")), &["/name"]).await;
}

#[tokio::test]
async fn a_number_is_held_to_its_inclusive_and_exclusive_bounds() {
    refuses::<Order>(order_with("seats", json!(0)), &["/seats"]).await;
    refuses::<Order>(order_with("seats", json!(11)), &["/seats"]).await;
    refuses::<Order>(order_with("ratio", json!(0)), &["/ratio"]).await;
    refuses::<Order>(order_with("ratio", json!(1)), &["/ratio"]).await;
    refuses::<Order>(order_with("step", json!(12)), &["/step"]).await;
    admits::<Order>(order_with("step", json!(-15))).await;
}

#[tokio::test]
async fn an_array_is_held_to_its_length_and_uniqueness() {
    refuses::<Order>(order_with("tags", json!([])), &["/tags"]).await;
    refuses::<Order>(order_with("tags", json!(["a", "b", "c"])), &["/tags"]).await;
    refuses::<Order>(order_with("tags", json!(["a", "a"])), &["/tags"]).await;
}

#[tokio::test]
async fn an_optional_value_is_bounded_only_when_present() {
    refuses::<Order>(order_with("nickname", json!("abcd")), &["/nickname"]).await;
}

#[tokio::test]
async fn a_nested_bound_is_reported_at_the_member_that_broke_it() {
    refuses::<Order>(
        order_with("first", json!({ "quantity": 0 })),
        &["/first/quantity"],
    )
    .await;
    refuses::<Order>(
        order_with("lines", json!([{ "quantity": 1 }, { "quantity": 0 }])),
        &["/lines/1/quantity"],
    )
    .await;
    // RFC 6901 escapes `~` and `/` in a member name.
    refuses::<Order>(
        order_with("by/sku", json!({ "x~y": { "quantity": 0 } })),
        &["/by~1sku/x~0y/quantity"],
    )
    .await;
}

#[tokio::test]
async fn every_broken_bound_is_reported() {
    let mut document = order();
    document["name"] = json!("a");
    document["seats"] = json!(0);
    document["lines"] = json!([{ "quantity": 0 }]);
    refuses::<Order>(document, &["/lines/0/quantity", "/name", "/seats"]).await;
}

/// A newtype, which is its member's value on the wire.
#[derive(Debug, Schema, Deserialize)]
struct Sku(#[schema(min_length = 3)] String);

/// A tuple struct, whose members are positions in an array.
#[derive(Debug, Schema, Deserialize)]
struct Range(#[schema(minimum = 0)] i32, #[schema(maximum = 9)] i32);

/// A flattened member, whose members are the parent's own.
#[derive(Debug, Schema, Deserialize)]
struct Tagged {
    sku: Sku,
    range: Range,
    #[serde(flatten)]
    line: Line,
}

#[tokio::test]
async fn a_newtype_a_tuple_and_a_flattened_member_report_where_the_wire_puts_them() {
    let tagged = json!({ "sku": "abc", "range": [0, 9], "quantity": 1 });
    admits::<Tagged>(tagged.clone()).await;

    let mut short = tagged.clone();
    short["sku"] = json!("ab");
    refuses::<Tagged>(short, &["/sku"]).await;

    let mut out_of_range = tagged.clone();
    out_of_range["range"] = json!([-1, 10]);
    refuses::<Tagged>(out_of_range, &["/range/0", "/range/1"]).await;

    let mut flattened = tagged;
    flattened["quantity"] = json!(0);
    refuses::<Tagged>(flattened, &["/quantity"]).await;
}

/// A transparent struct, which is its one named member on the wire.
#[derive(Debug, Schema, Deserialize)]
#[serde(transparent)]
struct Code {
    value: String,
}

/// Whether `$ty` implements `$kind`: the inherent constant exists only under
/// the bound, and shadows the trait's where it does.
macro_rules! implements {
    ($ty:ty: $kind:path) => {{
        struct Probe<T: ?Sized>(std::marker::PhantomData<T>);
        // Each probe reads one of the two constants, so the other is unused.
        #[allow(dead_code)]
        trait Otherwise {
            const IMPLEMENTS: bool = false;
        }
        impl<T: ?Sized> Otherwise for Probe<T> {}
        #[allow(dead_code)]
        impl<T: ?Sized + $kind> Probe<T> {
            const IMPLEMENTS: bool = true;
        }
        <Probe<$ty>>::IMPLEMENTS
    }};
}

/// A derived type takes its member's kind only where it is that member on the
/// wire, so a bound written on a field of the type applies to the member: a
/// one-member tuple struct and a transparent struct do, and a struct with a
/// named member or a tuple of two, which are an object and an array, do not.
#[test]
fn only_a_type_that_is_its_member_on_the_wire_takes_its_kind() {
    use kynos::schema::constraints::{Numeric, Textual};

    assert!(implements!(Sku: Textual));
    assert!(implements!(Code: Textual));
    assert!(!implements!(Line: Numeric));
    assert!(!implements!(Range: Numeric));
}

/// Externally tagged, the default: a payload sits under its variant's name.
#[derive(Debug, Schema, Deserialize)]
#[serde(rename_all = "snake_case")]
enum External {
    Named {
        #[schema(maximum = 5)]
        size: u8,
    },
    Wrapped(Line),
    Pair(#[schema(min_length = 1)] String, Line),
}

/// Internally tagged: a variant's members sit beside the tag.
#[derive(Debug, Schema, Deserialize)]
#[serde(tag = "kind")]
enum Internal {
    Named {
        #[schema(maximum = 5)]
        size: u8,
    },
    Wrapped(Line),
}

/// Adjacently tagged: a payload sits under the content member.
#[derive(Debug, Schema, Deserialize)]
#[serde(tag = "kind", content = "body")]
enum Adjacent {
    Named {
        #[schema(maximum = 5)]
        size: u8,
    },
    Pair(#[schema(min_length = 1)] String, Line),
}

#[tokio::test]
async fn an_enum_reports_where_its_tagging_puts_the_payload() {
    admits::<External>(json!({ "named": { "size": 5 } })).await;
    refuses::<External>(json!({ "named": { "size": 6 } }), &["/named/size"]).await;
    refuses::<External>(
        json!({ "wrapped": { "quantity": 0 } }),
        &["/wrapped/quantity"],
    )
    .await;
    refuses::<External>(
        json!({ "pair": ["", { "quantity": 0 }] }),
        &["/pair/0", "/pair/1/quantity"],
    )
    .await;

    admits::<Internal>(json!({ "kind": "Named", "size": 5 })).await;
    refuses::<Internal>(json!({ "kind": "Named", "size": 6 }), &["/size"]).await;
    refuses::<Internal>(json!({ "kind": "Wrapped", "quantity": 0 }), &["/quantity"]).await;

    admits::<Adjacent>(json!({ "kind": "Named", "body": { "size": 5 } })).await;
    refuses::<Adjacent>(
        json!({ "kind": "Named", "body": { "size": 6 } }),
        &["/body/size"],
    )
    .await;
    refuses::<Adjacent>(
        json!({ "kind": "Pair", "body": ["", { "quantity": 1 }] }),
        &["/body/0"],
    )
    .await;
}

fn three() -> u32 {
    3
}

/// Members serde fills when the document leaves them out, which the emitted
/// schema therefore leaves out of `required`.
#[derive(Debug, Schema, Deserialize)]
struct Profile {
    /// `String::default()` breaks this bound itself.
    #[serde(default)]
    #[schema(min_length = 1)]
    name: String,
    /// The filled value meets this bound, so a value breaking it was sent.
    #[serde(default)]
    #[schema(max_length = 3)]
    code: String,
    #[serde(default = "three")]
    #[schema(minimum = 1)]
    retries: u32,
    /// The filled value breaks `min_length` alone, so a value breaking
    /// `max_length` was sent.
    #[serde(default)]
    #[schema(min_length = 1, max_length = 3)]
    label: String,
    /// The filled value breaks `minimum` alone, so a value breaking it and
    /// `multiple_of` too was sent, although both are reported at one place.
    #[serde(default)]
    #[schema(minimum = 1, multiple_of = 2)]
    parity: i32,
    /// The same, one member down: the filled value's own member breaks
    /// `minimum` alone.
    #[serde(default)]
    stride: Stride,
    /// The same through an alias, whose failures are all moved to the
    /// object holding it.
    #[serde(default)]
    tally: Tally,
    /// The same through a set, whose members' failures are all moved to the
    /// set: the filled set's one member breaks `minimum` alone.
    #[serde(default = "evens")]
    evens: BTreeSet<Even>,
}

#[derive(Debug, Default, Schema, Deserialize)]
struct Stride {
    #[schema(minimum = 1, multiple_of = 2)]
    step: i32,
}

#[derive(Debug, Default, Schema, Deserialize)]
struct Tally {
    #[serde(alias = "n")]
    #[schema(minimum = 1, multiple_of = 2)]
    count: i32,
}

#[derive(Debug, Schema, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
struct Even {
    #[schema(minimum = 1, multiple_of = 2)]
    value: i32,
}

fn evens() -> BTreeSet<Even> {
    BTreeSet::from([Even { value: 0 }])
}

/// A container default whose filled value breaks a member's bound.
#[derive(Debug, Default, Schema, Deserialize)]
#[serde(default)]
struct Paging {
    #[schema(minimum = 1)]
    page: u32,
}

/// A container default whose filled value meets the member's bound.
#[derive(Debug, Schema, Deserialize)]
#[serde(default)]
struct Window {
    #[schema(minimum = 1)]
    size: u32,
}

impl Default for Window {
    fn default() -> Self {
        Self { size: 10 }
    }
}

/// A generic container: `Vec<T>` is `Default` whatever `T` is, so the check
/// reaches the value `items` is filled with, and `extra`'s is out of reach
/// where nothing bounds `T` by `Default`, which still compiles.
#[derive(Debug, Schema, Deserialize)]
struct Envelope<T> {
    #[serde(default)]
    #[schema(min_items = 1)]
    items: Vec<T>,
    #[serde(default)]
    extra: T,
}

#[tokio::test]
async fn a_member_serde_filled_is_not_refused_for_its_filled_value() {
    admits::<Profile>(json!({})).await;
    admits::<Paging>(json!({})).await;
    admits::<Window>(json!({})).await;
    admits::<Envelope<String>>(json!({})).await;
}

#[tokio::test]
async fn a_defaulted_member_is_held_to_its_bounds_where_its_filled_value_meets_them() {
    refuses::<Profile>(json!({ "code": "abcd" }), &["/code"]).await;
    refuses::<Profile>(json!({ "retries": 0 }), &["/retries"]).await;
    refuses::<Profile>(json!({ "label": "abcdef" }), &["/label"]).await;
    refuses::<Profile>(json!({ "parity": -1 }), &["/parity"]).await;
    refuses::<Profile>(json!({ "stride": { "step": -1 } }), &["/stride/step"]).await;
    refuses::<Profile>(json!({ "tally": { "n": -1 } }), &["/tally"]).await;
    refuses::<Profile>(json!({ "evens": [{ "value": -1 }] }), &["/evens"]).await;
    refuses::<Window>(json!({ "size": 0 }), &["/size"]).await;
}

/// A field and a variant serde also reads under an `alias`.
#[derive(Debug, Schema, Deserialize)]
struct Account {
    #[serde(alias = "nick")]
    #[schema(max_length = 3)]
    handle: String,
}

#[derive(Debug, Schema, Deserialize)]
struct Holder {
    account: Account,
}

#[derive(Debug, Schema, Deserialize)]
enum Shape {
    #[serde(alias = "sq")]
    Square {
        #[schema(maximum = 5)]
        side: u8,
    },
}

#[tokio::test]
async fn a_member_read_under_an_alias_is_reported_at_the_object_holding_it() {
    // Which name the document used is gone once it is read, so the pointer
    // names the object, which the document does hold.
    refuses::<Account>(json!({ "nick": "abcd" }), &[""]).await;
    refuses::<Account>(json!({ "handle": "abcd" }), &[""]).await;
    refuses::<Holder>(json!({ "account": { "nick": "abcd" } }), &["/account"]).await;
    refuses::<Shape>(json!({ "sq": { "side": 6 } }), &[""]).await;
    admits::<Shape>(json!({ "Square": { "side": 5 } })).await;
}

/// A map key bounded through `MapKey::key_constraints`, which the description
/// emits as `propertyNames`.
#[derive(Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
struct Code3(String);

impl SchemaTrait for Code3 {
    fn schema(registry: &mut kynos::schema::registry::Registry) -> kynos::openapi::Schema {
        String::schema(registry)
    }
}

impl kynos::schema::MapKey for Code3 {
    fn key_constraints() -> kynos::schema::constraints::Constraints {
        let mut constraints = kynos::schema::constraints::Constraints::default();
        constraints.min_length = Some(2);
        constraints.max_length = Some(3);
        constraints
    }

    fn as_member(&self) -> Option<&str> {
        Some(&self.0)
    }
}

#[derive(Debug, Schema, Deserialize)]
struct Catalogue {
    by_code: BTreeMap<Code3, Line>,
}

#[tokio::test]
async fn a_map_key_is_held_to_its_property_names_at_the_map() {
    admits::<Catalogue>(
        json!({ "by_code": { "ab": { "quantity": 1 }, "abc": { "quantity": 1 } } }),
    )
    .await;
    // Lengths count code points here too.
    admits::<Catalogue>(json!({ "by_code": { "ééé": { "quantity": 1 } } })).await;
    // A key has no location of its own, since a pointer to it names its
    // value, so it is reported at the map.
    refuses::<Catalogue>(
        json!({ "by_code": { "abcd": { "quantity": 1 } } }),
        &["/by_code"],
    )
    .await;
    refuses::<Catalogue>(
        json!({ "by_code": { "a": { "quantity": 1 } } }),
        &["/by_code"],
    )
    .await;
    refuses::<Catalogue>(
        json!({ "by_code": { "abcd": { "quantity": 0 } } }),
        &["/by_code", "/by_code/abcd/quantity"],
    )
    .await;
}

/// `Form<T>` runs the same check after `serde_urlencoded` reads the body.
#[cfg(feature = "form")]
mod form {
    use kynos::{
        Schema,
        error::rejection::BodyRejection,
        extract::{FromRequest, body::form::Form},
        http::{HeaderValue, Request, body::Body, header},
    };
    use serde::Deserialize;

    #[derive(Debug, Schema, Deserialize)]
    struct Signup {
        #[schema(min_length = 2)]
        name: String,
        #[schema(maximum = 10)]
        seats: u32,
    }

    async fn read(body: &'static str) -> Result<Signup, BodyRejection> {
        let mut request =
            Request::new(Body::from_bytes(bytes::Bytes::from_static(body.as_bytes())));
        request.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        Form::<Signup>::from_request(request, &())
            .await
            .map(|Form(signup)| signup)
    }

    #[tokio::test]
    async fn a_form_body_is_held_to_the_same_bounds() {
        let signup = read("name=ab&seats=10").await.expect("inside every bound");
        assert_eq!((signup.name.as_str(), signup.seats), ("ab", 10));

        match read("name=a&seats=11").await {
            Err(BodyRejection::Schema { failures }) => {
                assert_eq!(
                    failures.into_keys().collect::<Vec<_>>(),
                    ["/name", "/seats"]
                );
            }
            other => panic!("a form breaking two bounds was not refused at both: {other:?}"),
        }
    }
}

/// `JsonLines<Records<T>>` and `JsonSeq<Records<T>>` run the same check on
/// each record as it is yielded, keyed under the record's position: OpenAPI
/// 3.2 reads a sequential media type as an array in the same order.
#[cfg(feature = "openapi32")]
mod records {
    use kynos::{
        error::rejection::BodyRejection,
        extract::{
            FromRequest,
            body::json_lines::{JsonLines, JsonSeq, records::Records},
        },
        http::{HeaderValue, Request, body::Body, header},
    };
    use serde_json::{Value, json};

    use super::{Line, emitted};

    /// A body carrying `documents`, framed as `media_type` frames them.
    fn request(media_type: &'static str, documents: &[Value]) -> Request {
        let mut bytes = Vec::new();
        for document in documents {
            if media_type == "application/json-seq" {
                bytes.push(0x1e);
            }
            bytes.extend(serde_json::to_vec(document).expect("a document serializes"));
            bytes.push(b'\n');
        }
        let mut request = Request::new(Body::from_bytes(bytes::Bytes::from(bytes)));
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(media_type));
        request
    }

    /// Each record's outcome: `None` where admitted, and the pointers it was
    /// refused at otherwise.
    async fn outcomes(mut records: Records<Line>) -> Vec<Option<Vec<String>>> {
        let mut outcomes = Vec::new();
        while let Some(record) = records.next().await {
            outcomes.push(match record {
                Ok(_) => None,
                Err(BodyRejection::Schema { failures }) => Some(failures.into_keys().collect()),
                Err(other) => panic!("a well-formed record refused as something else: {other:?}"),
            });
        }
        outcomes
    }

    /// Holds each framing to the validator over the item schema, record by
    /// record, and returns where the records were refused.
    async fn agree(documents: &[Value]) -> Vec<Option<Vec<String>>> {
        let schema = emitted::<Line>();
        let validator = jsonschema::draft202012::new(&schema)
            .expect("an emitted schema compiles as draft 2020-12");

        let JsonLines { items } = JsonLines::<Records<Line>>::from_request(
            request("application/x-ndjson", documents),
            &(),
        )
        .await
        .expect("an NDJSON body is taken unread");
        let lines = outcomes(items).await;
        let JsonSeq { items } =
            JsonSeq::<Records<Line>>::from_request(request("application/json-seq", documents), &())
                .await
                .expect("a JSON text sequence is taken unread");
        let sequence = outcomes(items).await;

        assert_eq!(lines, sequence, "the two framings disagree");
        for (document, outcome) in documents.iter().zip(&lines) {
            assert_eq!(
                validator.is_valid(document),
                outcome.is_none(),
                "the validator and the record disagree on {document}: refused at {outcome:?}"
            );
        }
        lines
    }

    #[tokio::test]
    async fn each_record_is_held_to_its_bounds_and_reading_continues() {
        let refused = agree(&[
            json!({ "quantity": 1 }),
            json!({ "quantity": 0 }),
            json!({ "quantity": 2 }),
        ])
        .await;
        assert_eq!(refused, [None, Some(vec!["/1/quantity".to_owned()]), None]);
    }
}

/// `QueryString<T, Json>` runs the same check on the document it decoded.
#[cfg(feature = "openapi32")]
mod query_string {
    use kynos::{
        error::rejection::QueryRejection,
        extract::{FromRequestParts, params::querystring::QueryString},
        http::{Request, StatusCode, Uri, body::Body, media},
    };
    use serde_json::json;

    use super::{Line, emitted};

    /// What `QueryString<Line, Json>` makes of the query in `uri`.
    async fn read(uri: &'static str) -> Result<QueryString<Line, media::Json>, QueryRejection> {
        let mut request = Request::new(Body::empty());
        *request.uri_mut() = Uri::from_static(uri);
        let mut parts = request.into_parts().0;
        QueryString::<Line, media::Json>::from_request_parts(&mut parts, &()).await
    }

    #[tokio::test]
    async fn a_query_string_is_held_to_the_same_bounds() {
        let schema = emitted::<Line>();
        let validator = jsonschema::draft202012::new(&schema)
            .expect("an emitted schema compiles as draft 2020-12");

        assert!(validator.is_valid(&json!({ "quantity": 1 })));
        read("/search?%7B%22quantity%22%3A1%7D")
            .await
            .expect("inside every bound");

        assert!(!validator.is_valid(&json!({ "quantity": 0 })));
        let rejection = read("/search?%7B%22quantity%22%3A0%7D")
            .await
            .expect_err("a broken bound is refused");
        assert_eq!(rejection.status(), StatusCode::BAD_REQUEST);
        match rejection {
            QueryRejection::Schema { name, failures } => {
                assert_eq!(name, "querystring");
                assert_eq!(failures.into_keys().collect::<Vec<_>>(), ["/quantity"]);
            }
            other => panic!("a broken bound was refused as something else: {other:?}"),
        }
    }
}

/// `MultipartForm<T>` runs the same check on the value its parts built, each
/// part sitting under its field's name.
#[cfg(feature = "multipart")]
mod multipart {
    use kynos::{
        Schema,
        error::rejection::BodyRejection,
        extract::{FromRequest, body::multipart::MultipartForm},
        http::{HeaderValue, Request, body::Body, header},
    };

    #[derive(Debug, Schema, kynos::MultipartForm)]
    struct Upload {
        #[schema(max_length = 3)]
        name: String,
    }

    /// What `MultipartForm<Upload>` makes of one `name` part holding `value`.
    async fn read(value: &str) -> Result<Upload, BodyRejection> {
        let body = format!(
            "--x\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\n{value}\r\n--x--\r\n"
        );
        let mut request = Request::new(Body::from_bytes(bytes::Bytes::from(body)));
        request.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("multipart/form-data; boundary=x"),
        );
        MultipartForm::<Upload>::from_request(request, &())
            .await
            .map(|MultipartForm(upload)| upload)
    }

    #[tokio::test]
    async fn a_multipart_body_is_held_to_the_same_bounds() {
        let upload = read("abc").await.expect("inside every bound");
        assert_eq!(upload.name, "abc");

        match read("abcd").await {
            Err(BodyRejection::Schema { failures }) => {
                assert_eq!(failures.into_keys().collect::<Vec<_>>(), ["/name"]);
            }
            other => panic!("a part breaking its bound was not refused at it: {other:?}"),
        }
    }
}
