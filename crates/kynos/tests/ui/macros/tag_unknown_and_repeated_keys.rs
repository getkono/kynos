//! The `#[tag(...)]` grammar is closed and reads each key once: a misspelled
//! key once dropped the description it carried, and a second `name` silently
//! replaced the first.

#[derive(kynos::Tag)]
#[tag(descripton = "Managing user accounts")]
struct Users;

#[derive(kynos::Tag)]
#[tag(name = "orders")]
#[tag(name = "purchases")]
struct Orders;

fn main() {}
