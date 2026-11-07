//! What `name_instantiation!` refuses: an alias naming no one instantiation,
//! an attribute that would apply to the alias alone, and a name no component
//! can carry. The accepted form is `ui/pass/name_instantiation.rs`.

#[derive(kynos::Schema)]
struct Page<T> {
    items: Vec<T>,
}

#[derive(kynos::Schema)]
struct Torrent {
    name: String,
}

kynos::name_instantiation!(Pages<T> = Page<T>);

kynos::name_instantiation! {
    #[cfg(test)]
    TorrentPage = Page<Torrent>;
}

kynos::name_instantiation!(Séite = Page<Torrent>);

fn main() {}
