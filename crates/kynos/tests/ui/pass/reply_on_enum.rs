#[derive(kynos::Reply)]
enum Accepted {
    #[reply(status = 201)]
    Created(u32),
    #[reply(status = 202)]
    Queued,
}

fn main() {}
