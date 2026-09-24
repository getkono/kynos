//! A tuple member serde may leave out of what it writes, with another position
//! after it: `Reading(0, "s".into())` writes `["s"]`, a string where the first
//! position is an integer. serde accepts this declaration, both members having
//! a default, so the refusal is the derive's own. The control is
//! `pass/schema_tuple_member_skipped_if_last`.

fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Reading(
    #[serde(default, skip_serializing_if = "is_zero")] u64,
    #[serde(default)] String,
);

fn main() {}
