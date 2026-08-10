//! `u128` and `i128`: outside JSON's safe integer range, and no OAS format.

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<u128>();
}
