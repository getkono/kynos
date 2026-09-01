//! The control for `macros/route_tag_repeated`: the same route, differing only
//! in that it names one tag.

#[derive(kynos::Tag)]
struct Users;

#[derive(kynos::Tag)]
struct Admin;

#[kynos::get("/users", tag = Users)]
async fn list() {}

fn main() {
    use kynos::router::endpoint::meta::EndpointMeta;

    assert_eq!(<list as EndpointMeta>::TAGS.len(), 1);
    assert_eq!(<list as EndpointMeta>::TAGS[0].name(), "Users");
    let _ = <Admin as kynos::router::operation::Tag>::NAME;
}
