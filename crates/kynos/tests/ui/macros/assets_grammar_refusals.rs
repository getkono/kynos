//! The `assets!` grammar is a unit struct naming the set and its keys: `dir`,
//! which is required, `exclude` and `warn_over`. Each rule is read before the
//! directory is, so none of these needs one to exist.
//!
//! The control is the set `tests/assets.rs` declares rather than a `pass/`
//! case: a set needs a directory, and a `pass/` case is built from a project
//! under the target directory, from which no committed path is portable.

kynos::assets! {
    dir = "no-such-directory",
}

kynos::assets! {
    struct Dirless;
}

kynos::assets! {
    struct Misspelled;
    dir = "no-such-directory",
    nonsense = "x",
}

kynos::assets! {
    struct Sized;
    dir = "no-such-directory",
    warn_over = "4KB",
}

kynos::assets! {
    struct Excluding;
    dir = "no-such-directory",
    exclude = [""],
}

fn main() {}
