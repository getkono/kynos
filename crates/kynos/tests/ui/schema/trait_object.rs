//! `Box<dyn Trait>`: no schema exists.

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<Box<dyn std::fmt::Debug>>();
}
