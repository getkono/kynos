//! `usize` and `isize`: the width depends on the build target.

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<usize>();
}
