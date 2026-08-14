//! A type that is not an `async fn` of extractors is not a `Handler`.

struct NotAHandler;

fn is_handler<C, A, H: kynos::handler::Handler<C, A>>(_: H) {}

fn main() {
    is_handler::<(), (), _>(NotAHandler);
}
