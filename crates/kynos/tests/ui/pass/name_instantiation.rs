// Two instantiations named in one invocation, one through a raw identifier,
// which `Router::name_instantiation` registers. The refused forms of the same
// declaration are in `ui/macros/name_instantiation_refusals.rs`, and the same
// registration without the macro in `ui/traits/named_instantiation.rs`.
#[derive(kynos::Schema)]
struct Page<T> {
    items: Vec<T>,
}

#[derive(kynos::Schema)]
struct Torrent {
    name: String,
}

#[derive(kynos::Schema)]
struct Plan {
    steps: u32,
}

kynos::name_instantiation! {
    /// One page of torrents.
    pub TorrentPage = Page<Torrent>;
    pub(crate) r#PlanPage = Page<Plan>;
}

fn main() {
    let _ = kynos::Router::<()>::new()
        .name_instantiation::<TorrentPage>()
        .name_instantiation::<PlanPage>();
}
