//! A path reaching some other type than the struct is reported at the path,
//! where it is written.

#[derive(Clone)]
struct Pool;

struct App {
    services: Services,
    name: &'static str,
}

#[derive(kynos::Provider)]
#[provider(for = App, via = name)]
struct Services {
    pool: Pool,
}

fn main() {}
