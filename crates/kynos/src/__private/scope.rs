//! The probe by which the `Scope` derive refuses a `#[scope(rename_all)]` or
//! `#[scope(rename)]` on an enum serde names: the derive reads serde's names,
//! and a second one for a scope would be one serde neither writes nor reads.
//!
//! serde is probed by autoref specialization, as in [`query`](super::query):
//! `(&&Enum::<T>::new()).by_serde()` resolves to [`NamedBySerde`] where `T`
//! implements `Serialize`, then where it implements `DeserializeOwned`, and to
//! [`UnnamedBySerde`] where it implements neither. Only the latter implements
//! [`NamedOnce`], so an enum with no serde, which `#[scope(...)]` names,
//! compiles.

use std::marker::PhantomData;

/// A scope enum, whose serde [`ProbeWritten`], [`ProbeRead`] and
/// [`ProbeUnnamed`] probe.
pub struct Enum<T>(PhantomData<T>);

impl<T> Enum<T> {
    /// The probe of `T`.
    #[must_use]
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<T> Default for Enum<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> std::fmt::Debug for Enum<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Enum(..)")
    }
}

/// An enum serde writes or reads, so serde names its scopes.
#[derive(Debug)]
pub struct NamedBySerde;

/// An enum serde neither writes nor reads, so `#[scope(...)]` names its
/// scopes.
#[derive(Debug)]
pub struct UnnamedBySerde;

/// Resolves a `Serialize` enum to [`NamedBySerde`], first.
pub trait ProbeWritten {
    /// That serde writes the enum.
    fn by_serde(&self) -> NamedBySerde {
        NamedBySerde
    }
}

impl<T: serde::Serialize> ProbeWritten for &Enum<T> {}

/// Resolves a `DeserializeOwned` enum to [`NamedBySerde`], after
/// [`ProbeWritten`].
pub trait ProbeRead {
    /// That serde reads the enum.
    fn by_serde(&self) -> NamedBySerde {
        NamedBySerde
    }
}

impl<T: serde::de::DeserializeOwned> ProbeRead for &&Enum<T> {}

/// The fallback of [`ProbeWritten`] and [`ProbeRead`], for an enum with
/// neither.
pub trait ProbeUnnamed {
    /// That serde does not name the enum.
    fn by_serde(&self) -> UnnamedBySerde {
        UnnamedBySerde
    }
}

impl<T> ProbeUnnamed for Enum<T> {}

/// What an enum carrying `#[scope(rename_all)]` or `#[scope(rename)]` must
/// be: one serde does not name.
#[diagnostic::on_unimplemented(
    message = "serde names this enum's variants, so `#[scope(...)]` would name a scope by a \
               name serde neither writes nor reads",
    label = "a second name for a scope serde names",
    note = "`Scope` reads serde's names: write `#[serde(rename_all = \"...\")]` or \
            `#[serde(rename = \"...\")]` in place of `#[scope(...)]`",
    note = "a hand-written `Serialize` or `Deserialize` is refused too, since the derive cannot \
            read the names it writes: derive serde instead, or drop `#[derive(Scope)]` and \
            implement `Scope` by hand"
)]
pub trait NamedOnce {}

impl NamedOnce for UnnamedBySerde {}

/// Refuses, where `probe` is [`NamedBySerde`], a `#[scope(...)]` key.
pub fn named_once<P: NamedOnce>(probe: &P) {
    let _ = probe;
}
