// A concrete type naming itself is named, so the field is a `$ref` to the
// component being defined and the description ends. The generic form of the
// same declaration is refused: see `ui/macros/schema_recursive_generic.rs`.
#[derive(kynos::Schema)]
struct Node {
    value: u32,
    next: Option<Box<Node>>,
}

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<Node>();
}
