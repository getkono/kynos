use kynos::router::group::Group;

fn main() {
    let _ = Group::<()>::new("/v1").catch_panics();
}
