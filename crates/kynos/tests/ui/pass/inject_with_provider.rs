#[derive(Clone)]
struct Db;

#[derive(kynos::Provider)]
struct App {
    db: Db,
}

fn resolvable<C: kynos::di::Provides<Db>>() {}

fn main() {
    resolvable::<App>();
}
