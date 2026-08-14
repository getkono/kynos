fn describes<T: kynos::extract::describe::Describe>() {}

fn main() {
    describes::<kynos::di::inject::Inject<u32>>();
}
