use kynos::response::status::NoContent;

async fn health() -> NoContent {
    NoContent
}

fn is_handler<C, A, H: kynos::handler::Handler<C, A>>(_: H) {}

fn main() {
    is_handler::<(), _, _>(health);
}
