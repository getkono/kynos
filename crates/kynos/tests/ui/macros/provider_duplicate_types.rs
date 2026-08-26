#[derive(Clone)]
struct Pool;

#[derive(kynos::Provider)]
struct App {
    primary: Pool,
    replica: Pool,
}

fn main() {}
