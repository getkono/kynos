//! A type's own `#[schema(...)]` takes `constraint_type` alone, and once: a
//! bound written there would read as enforced and be neither described nor
//! checked, and a second type would silently replace the first.

#[derive(kynos::Schema)]
#[schema(minimum = 1)]
struct Bounded {
    total: u32,
}

#[derive(kynos::Schema)]
#[schema(constraint_type = "https://errors.example.com/a")]
#[schema(constraint_type = "https://errors.example.com/b")]
struct Repeated {
    total: u32,
}

fn main() {}
