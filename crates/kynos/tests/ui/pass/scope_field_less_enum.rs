//! The control for `macros/scope_shape_refusals`: a field-less enum, every
//! variant a unit, names a scope per variant.

#[derive(Clone, Copy, kynos::Scope)]
enum Grant {
    Admin,
    Read,
}

fn scope<T: kynos::security::Scope>() {}

fn main() {
    scope::<Grant>();
}
