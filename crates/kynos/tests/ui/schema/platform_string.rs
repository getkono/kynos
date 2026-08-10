//! `PathBuf` and `OsString`: platform-dependent and not guaranteed UTF-8.

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<std::path::PathBuf>();
}
