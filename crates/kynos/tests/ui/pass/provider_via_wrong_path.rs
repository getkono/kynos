//! The control for `macros/provider_via_wrong_path`: the same context, the
//! path naming the field that holds the struct.

#[derive(Clone)]
struct Pool;

struct App {
    services: Services,
    #[allow(dead_code)]
    name: &'static str,
}

#[derive(kynos::Provider)]
#[provider(for = App, via = services)]
struct Services {
    pool: Pool,
}

fn main() {}
