//! A type that is not an extractor does not describe itself.

struct NotAnExtractor;

fn describes<T: kynos::extract::describe::Describe>() {}

fn main() {
    describes::<NotAnExtractor>();
}
