//! A group of fixed fields that is only ever constructed and sent, as a
//! `Stamp`'s is, raises no `dead_code` warning: its encoder reads each field,
//! which it cannot do without.

#![deny(dead_code)]

use kynos::extract::params::header::EncodeHeaders;

#[derive(kynos::HeaderParams)]
struct Hardened {
    #[header(rename = "X-Content-Type-Options", value = "nosniff")]
    nosniff: (),
    #[header(rename = "Cache-Control", value = "no-store")]
    no_store: Option<()>,
}

fn main() {
    let _ = Hardened {
        nosniff: (),
        no_store: None,
    }
    .encode();
}
