//! A scheme that describes itself and never says where its credential travels.
use kynos::security::{SecurityScheme, auth::Auth, carrier::Carries};

struct Undeclared;

impl SecurityScheme for Undeclared {
    const NAME: &'static str = "Undeclared";
    type Credential = String;

    fn describe() -> kynos::openapi::SecurityScheme {
        kynos::openapi::SecurityScheme::bearer(None)
    }
}

fn carries<S: Carries>() {}

fn main() {
    carries::<Undeclared>();
    let _ = std::marker::PhantomData::<Auth<Undeclared>>;
}
