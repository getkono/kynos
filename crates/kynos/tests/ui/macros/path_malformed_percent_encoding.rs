// Control: `pass/path_pchar_literal.rs`, whose `%20` is a well-formed triple.
fn main() {
    kynos::path!("/users/a%2gb");
}
