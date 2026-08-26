fn short_circuits<T: kynos::response::ShortCircuit>() {}

fn main() {
    short_circuits::<std::convert::Infallible>();
}
