//! An externally tagged enum flattened into an object serde reads under
//! `deny_unknown_fields`: it is not `Flatten` at all, since its closed branch
//! would refuse the parent's own members, and so not `ClosedFlatten` either.
//! Both bounds are asserted, so both refusals are reported, and the second one
//! claims nothing about how serde reads the enum. The control is
//! `pass/schema_closed_beside_flattened_adjacently_tagged_enum`, whose enum is
//! adjacently tagged instead.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
enum Mode {
    Fixed { level: u8 },
}

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Thing {
    id: u64,
    #[serde(flatten)]
    mode: Mode,
}

fn main() {}
