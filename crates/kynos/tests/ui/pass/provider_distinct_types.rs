#[derive(Clone)]
struct Pool;

#[derive(Clone)]
struct Cache;

#[derive(kynos::Provider)]
struct App {
    primary: Pool,
    replica: Cache,
}

fn main() {}
