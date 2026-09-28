//! A split `rename` whose sides differ on a field serde both writes and reads:
//! serde writes it as `a` and reads it as `b`, so no one schema names it
//! truly. The control is `pass/schema_split_rename_one_way`.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Stamp {
    #[serde(rename(serialize = "a", deserialize = "b"))]
    at: u64,
}

fn main() {}
