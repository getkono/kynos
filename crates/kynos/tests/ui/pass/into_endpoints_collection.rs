fn mountable<C, E: kynos::router::endpoint::set::IntoEndpoints<C>>() {}

fn main() {
    mountable::<(), kynos::router::endpoint::set::Endpoints<()>>();
}
