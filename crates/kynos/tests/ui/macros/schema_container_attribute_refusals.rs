//! A type's own `#[schema(...)]` takes `constraint_problem` alone, and once:
//! a bound written there would read as enforced and be neither described nor
//! checked, and a second problem would silently replace the first. The
//! `constraint_type` key it replaced is refused naming it. A refused
//! derive emits nothing, so `A` and `B` are never resolved.

#[derive(kynos::Schema)]
#[schema(minimum = 1)]
struct Bounded {
    total: u32,
}

#[derive(kynos::Schema)]
#[schema(constraint_problem = A)]
#[schema(constraint_problem = B)]
struct Repeated {
    total: u32,
}

#[derive(kynos::Schema)]
#[schema(constraint_type = "https://errors.example.com/a")]
struct Replaced {
    total: u32,
}

fn main() {}
