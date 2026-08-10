#[derive(kynos::Schema)]
enum Kind {
    Read,
    Write,
}

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<Box<Kind>>();
}
