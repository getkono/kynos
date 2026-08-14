//! A type that is not a body extractor is not a request body.

struct NotABody;

fn is_content<T: kynos::extract::describe::RequestContent>() {}

fn main() {
    is_content::<NotABody>();
}
