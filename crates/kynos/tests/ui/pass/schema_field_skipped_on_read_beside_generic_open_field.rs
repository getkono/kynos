//! A generic open field beside a field serde never reads compiles once the
//! parameter is bounded by `AdmitsAny`, which the derive's witness carries
//! through the type's own `where` clause.

use kynos::schema::{flatten::AdmitsAny, unchecked::Unchecked};

#[derive(kynos::Schema, serde::Serialize)]
struct Thing<M>
where
    M: AdmitsAny,
{
    id: u64,
    #[serde(skip_deserializing)]
    stamp: u64,
    #[serde(flatten)]
    #[schema(open)]
    extra: M,
}

type Envelope = Thing<Unchecked<serde_json::Map<String, serde_json::Value>>>;

fn main() {
    let _ = std::mem::size_of::<Envelope>();
}
