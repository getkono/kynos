//! The control for `traits/open_map.rs`: both standard maps satisfy the bound,
//! and so does a `Box` or an `Arc` around one.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

fn is_open_map<T: kynos::schema::flatten::OpenMap>() {}

fn main() {
    is_open_map::<BTreeMap<String, String>>();
    is_open_map::<HashMap<String, u64>>();
    is_open_map::<Box<BTreeMap<String, String>>>();
    is_open_map::<Arc<HashMap<String, u64>>>();
}
