//! The control for `traits/rangeable.rs`: octets of a known length are
//! rangeable. Only the body type differs.

use kynos::{
    extract::{body::binary::Binary, media::OctetStream},
    response::range::rangeable::Rangeable,
};

fn is_rangeable<T: Rangeable>() {}

fn main() {
    is_rangeable::<Binary<OctetStream>>();
}
