//! The control for `macros/schema_tuple_member_skipped_if_not_last`: the same
//! two members in the other order, so the one serde may leave out is the last
//! position, and its default fills the shorter array on read.

fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Reading(
    #[serde(default)] String,
    #[serde(default, skip_serializing_if = "is_zero")] u64,
);

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<Reading>();
}
