#[derive(kynos::Reply)]
enum Accepted {
    Created(u32),
    Queued,
}

fn main() {}
