//! An interceptor's short circuit is a response, and this is not one.

struct NotAResponse;

fn short_circuits<T: kynos::response::ShortCircuit>() {}

fn main() {
    short_circuits::<NotAResponse>();
}
