//! A field's `#[schema(...)]` names each constraint unqualified and once: a
//! qualified key was skipped, dropping the bound it carried, and a second
//! `minimum` silently replaced the first.

#[derive(kynos::Schema)]
struct Qualified {
    #[schema(kynos::minimum = 1)]
    total: u32,
}

#[derive(kynos::Schema)]
struct Repeated {
    #[schema(minimum = 1)]
    #[schema(minimum = 2)]
    total: u32,
}

fn main() {}
