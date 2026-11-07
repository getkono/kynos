//! `#[schema(via = path)]` names the one function a field converts through
//! into the type its `as` describes it as, so it is written once per field and
//! only beside an `as`.
//!
//! The control is `a_field_converted_through_a_named_function_is_held_to_what_its_type_states`
//! in `tests/constraints.rs`: one `via` beside one `as`.

struct Hash([u8; 2]);

impl Hash {
    fn to_hex(&self) -> String {
        format!("{:02x}{:02x}", self.0[0], self.0[1])
    }

    fn to_upper_hex(&self) -> String {
        format!("{:02X}{:02X}", self.0[0], self.0[1])
    }
}

#[derive(kynos::Schema)]
struct Digest {
    #[schema(as = String, via = Hash::to_hex)]
    #[schema(min_length = 4, via = Hash::to_upper_hex)]
    hash: Hash,
}

#[derive(kynos::Schema)]
struct Checksum {
    #[schema(min_length = 4, via = Hash::to_hex)]
    hash: Hash,
}

fn main() {}
