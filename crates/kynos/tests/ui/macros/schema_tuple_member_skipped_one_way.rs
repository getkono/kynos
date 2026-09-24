//! A tuple member serde leaves out of what it writes and not out of what it
//! reads: `P(1, 7)` writes `[1]`, which serde refuses to read back, so no one
//! array schema is true of both. serde accepts this declaration, so the
//! refusal is the derive's own. The control is
//! `pass/schema_tuple_member_skipped_both_ways`.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct P(u64, #[serde(skip_serializing)] u64);

fn main() {}
