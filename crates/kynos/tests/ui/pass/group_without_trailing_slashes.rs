use kynos::router::group::Group;

fn main() {
    if std::hint::black_box(false) {
        let _ = Group::<()>::new("/v1").catch_panics();
    }
}
