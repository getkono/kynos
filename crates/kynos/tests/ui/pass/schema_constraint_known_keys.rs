//! The control for `macros/schema_constraint_qualified_key`: the same fields,
//! differing only in that each constraint is unqualified and written once.

#[derive(kynos::Schema)]
struct Qualified {
    #[schema(minimum = 1)]
    total: u32,
}

#[derive(kynos::Schema)]
struct Repeated {
    #[schema(minimum = 1)]
    #[schema(maximum = 2)]
    total: u32,
}

fn main() {}
