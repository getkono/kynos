//! A scope is one variant of a field-less enum: a struct has no variants to
//! name, and a variant's fields have nothing to say in a name. The control
//! is `pass/scope_field_less_enum`.

#[derive(Clone, Copy, kynos::Scope)]
struct Admin;

#[derive(Clone, Copy, kynos::Scope)]
enum Grant {
    Admin,
    Read(u8),
}

fn main() {}
