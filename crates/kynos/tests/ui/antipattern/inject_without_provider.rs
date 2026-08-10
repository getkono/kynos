//! README anti-pattern 7: a context that provides no `Db` cannot satisfy
//! `Inject<Db>`.

#[derive(Clone)]
struct Db;

struct App;

fn resolvable<C: kynos::di::Provides<Db>>() {}

fn main() {
    resolvable::<App>();
}
