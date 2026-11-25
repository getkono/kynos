//! `#[schema(string)]` describes the text `FromStr` reads and `Display`
//! writes, so a type implementing neither is refused at the key. The control
//! is `pass/schema_string`.

#[derive(kynos::Schema)]
#[schema(string)]
struct Unread(u32);

fn main() {}
