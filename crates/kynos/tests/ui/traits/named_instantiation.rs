//! An instantiation nothing named cannot be registered under a name. The
//! named form is `ui/pass/name_instantiation.rs`.

#[derive(kynos::Schema)]
struct Page<T> {
    items: Vec<T>,
}

#[derive(kynos::Schema)]
struct Torrent {
    name: String,
}

fn main() {
    let _ = kynos::Router::<()>::new().name_instantiation::<Page<Torrent>>();
}
