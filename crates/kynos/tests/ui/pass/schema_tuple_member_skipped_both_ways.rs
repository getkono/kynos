//! The control for `macros/schema_tuple_member_skipped_one_way`: the same
//! member, differing only in that `skip` leaves it out of what serde reads too.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct P(u64, #[serde(skip)] u64);

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<P>();
}
