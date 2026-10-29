//! Reading a struct only from an object, at every position of a document, and
//! keying a failure at the position it happened.
//!
//! Shared by every input decoded as JSON — the `Json`, `JsonAs` and record
//! bodies, and the `QueryString` parameter — so each reads a document alike.

mod access;
mod key;

use std::cell::{Cell, RefCell};

use serde::de::{DeserializeSeed, Deserializer, EnumAccess, MapAccess, SeqAccess, Visitor};

use self::{
    access::{Elements, Members},
    key::Key,
};
use crate::schema::constraints::Pointer;

/// Reads one JSON document as [`serde_json::from_slice`] does, except that a
/// struct is read only from an object, wherever the document holds one, and a
/// failure says where it happened.
///
/// `serde`'s derive also reads a struct from an array of its members, which
/// the `type: object` its schema declares refuses; where every member
/// defaults, it reads `[]` as `{}`. The refusal comes before any member is
/// read, and reaches every position `serde_json` hands out: a member, a
/// sequence element, a map value, `Option`, a newtype and an enum variant.
/// What `serde` buffers before choosing a type — an untagged or internally
/// tagged enum, every member of a struct that flattens one — is `serde`'s to
/// read, and a failure in it is keyed at the buffered value. Bytes that are
/// not one JSON document keep their syntax error, so the 400/422 split holds.
pub(crate) fn from_slice<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, Failure> {
    let state = State::default();
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = T::deserialize(Strict {
        inner: &mut deserializer,
        state: &state,
    })
    .map_err(|error| Failure {
        // A syntax error wins: the refusal is only for well-formed JSON.
        error: if state.refused.get() {
            serde_json::from_slice::<serde::de::IgnoredAny>(bytes).err()
        } else {
            None
        }
        .unwrap_or(error),
        path: state.path.take(),
    })?;
    deserializer.end().map_err(|error| Failure {
        error,
        path: Vec::new(),
    })?;
    Ok(value)
}

/// Why a document did not read, and where in it.
#[derive(Debug)]
pub(crate) struct Failure {
    /// What `serde_json` reported.
    pub(crate) error: serde_json::Error,

    /// The steps from the document to the position that failed, the
    /// innermost first, as the error unwound through them.
    path: Vec<Step>,
}

impl Failure {
    /// The JSON Pointer of the position that failed, under `base`: where the
    /// document sits in what holds it.
    ///
    /// A member is named as its key read: an escaped key unescaped, and a
    /// key read as a number by that number as Rust writes it, so a `"1e2"`
    /// read as an `f64` is named `100`.
    pub(crate) fn pointer(&self, base: Pointer<'_>) -> String {
        /// Descends from `at` along `rest`, the outermost step last.
        fn render(at: Pointer<'_>, rest: &[Step]) -> String {
            match rest.split_last() {
                None => at.to_string(),
                Some((Step::Member(name), rest)) => render(at.member(name), rest),
                Some((Step::Index(index), rest)) => render(at.index(*index), rest),
            }
        }

        render(base, &self.path)
    }

    /// Whether the document was well-formed JSON that did not fit the type:
    /// serde's `Data` category, where every JSON input draws its 400/422 line.
    /// The rest are bytes that are not JSON.
    pub(crate) fn is_schema_failure(&self) -> bool {
        match self.error.classify() {
            serde_json::error::Category::Data => true,
            serde_json::error::Category::Io
            | serde_json::error::Category::Syntax
            | serde_json::error::Category::Eof => false,
        }
    }
}

/// One step of a failure's path.
#[derive(Debug)]
enum Step {
    Member(String),
    Index(usize),
}

/// What every position of one document shares.
#[derive(Debug, Default)]
struct State {
    /// Set when an array was refused, so a syntax error later in the
    /// document can still win.
    refused: Cell<bool>,

    /// The names of the keys being read that could not be borrowed, as a
    /// stack: each map and sequence truncates it to where it began before
    /// its next member or element.
    keys: RefCell<String>,

    /// The path an error has unwound through so far; empty unless one is
    /// unwinding.
    path: RefCell<Vec<Step>>,
}

impl State {
    /// Passes `result` on, adding `step` to the path of an error unwinding
    /// through it.
    ///
    /// A position that read forgets a path a value it read swallowed the
    /// error of, so that path is not taken for the next error's. An error a
    /// visitor returns straight after a read failed keeps that read's path:
    /// `serde`'s errors carry no identity, so one the visitor made in its
    /// place cannot be told from the read's own passed on, and the read is
    /// where the document went wrong.
    fn at<T, E>(&self, result: Result<T, E>, step: impl FnOnce() -> Option<Step>) -> Result<T, E> {
        match result {
            Ok(value) => {
                self.path.borrow_mut().clear();
                Ok(value)
            }
            Err(error) => {
                if let Some(step) = step() {
                    self.path.borrow_mut().push(step);
                }
                Err(error)
            }
        }
    }

    /// Forgets a path a swallowed error left, before a read whose own
    /// failure would otherwise unwind onto it.
    fn begin(&self) {
        self.path.borrow_mut().clear();
    }

    /// Where the key buffer is now, for a map or sequence to truncate it to.
    fn mark(&self) -> usize {
        self.keys.borrow().len()
    }
}

/// A position in a document, refusing to read a struct from an array, and
/// handing the same refusal to every position it hands out: as the
/// deserializer, seed and enum accessor `serde_json` passes in.
///
/// Each wrapper adds a pointer or two, and what one costs per element is the
/// forwarding the compiler inlines and the key a member holds. A document
/// that reads allocates nothing for them but the one key buffer its escaped
/// keys share; a failure's path is built only as the failure unwinds.
struct Strict<'a, D> {
    /// What every request is forwarded to.
    inner: D,

    /// The document's shared state.
    state: &'a State,
}

/// Forwards each named `deserialize_*` method, with its extra arguments, to
/// the inner deserializer, the visitor wrapped where the method says `wrap`.
macro_rules! forward {
    ($($method:ident($($argument:ident: $kind:ty),*) $wrap:tt;)*) => {$(
        fn $method<V: Visitor<'de>>(
            self,
            $($argument: $kind,)*
            visitor: V,
        ) -> Result<V::Value, Self::Error> {
            let visitor = forward!(@visitor $wrap visitor self.state);
            self.inner.$method($($argument,)* visitor)
        }
    )*};
    (@visitor raw $visitor:ident $state:expr) => { $visitor };
    (@visitor wrap $visitor:ident $state:expr) => {
        Visit::new($visitor, $state, false, None)
    };
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for Strict<'_, D> {
    type Error = D::Error;

    forward! {
        deserialize_any() wrap;
        deserialize_bool() raw;
        deserialize_i8() raw;
        deserialize_i16() raw;
        deserialize_i32() raw;
        deserialize_i64() raw;
        deserialize_i128() raw;
        deserialize_u8() raw;
        deserialize_u16() raw;
        deserialize_u32() raw;
        deserialize_u64() raw;
        deserialize_u128() raw;
        deserialize_f32() raw;
        deserialize_f64() raw;
        deserialize_char() raw;
        deserialize_str() raw;
        deserialize_string() raw;
        deserialize_bytes() raw;
        deserialize_byte_buf() raw;
        deserialize_option() wrap;
        deserialize_unit() raw;
        deserialize_unit_struct(name: &'static str) raw;
        deserialize_seq() wrap;
        deserialize_tuple(len: usize) wrap;
        deserialize_tuple_struct(name: &'static str, len: usize) wrap;
        deserialize_map() wrap;
        deserialize_enum(name: &'static str, variants: &'static [&'static str]) wrap;
        deserialize_identifier() raw;
        deserialize_ignored_any() raw;
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let visitor = Visit::new(visitor, self.state, true, None);
        self.inner.deserialize_struct(name, fields, visitor)
    }

    /// A newtype is its member, as `serde_json` reads it; `serde_json`'s own
    /// private tokens, such as `RawValue`'s, are its to read.
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if name.starts_with("$serde_json::") {
            self.inner.deserialize_newtype_struct(name, visitor)
        } else {
            let visitor = Visit::new(visitor, self.state, false, None);
            self.inner.deserialize_newtype_struct(name, visitor)
        }
    }

    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }
}

/// A visitor whose nested positions are [`Strict`], refusing a sequence where
/// it was asked for a struct.
struct Visit<'a, V> {
    visitor: V,
    state: &'a State,

    /// Whether the value asked for is a struct, which an array is not.
    object: bool,

    /// Set once a sequence or map is visited, for a variant's content to
    /// tell a failure inside it from content of the wrong kind.
    entered: Option<&'a Cell<bool>>,
}

impl<'a, V> Visit<'a, V> {
    /// Wraps `visitor`, asked for a struct where `object` says so.
    fn new(visitor: V, state: &'a State, object: bool, entered: Option<&'a Cell<bool>>) -> Self {
        Self {
            visitor,
            state,
            object,
            entered,
        }
    }

    /// Notes that the content was reached.
    fn enter(&self) {
        if let Some(entered) = self.entered {
            entered.set(true);
        }
    }
}

/// Forwards each named scalar `visit_*` method to the inner visitor.
macro_rules! visit {
    ($($method:ident($kind:ty);)*) => {$(
        fn $method<E: serde::de::Error>(self, value: $kind) -> Result<Self::Value, E> {
            self.visitor.$method(value)
        }
    )*};
}

impl<'de, V: Visitor<'de>> Visitor<'de> for Visit<'_, V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.visitor.expecting(formatter)
    }

    visit! {
        visit_bool(bool);
        visit_i8(i8);
        visit_i16(i16);
        visit_i32(i32);
        visit_i64(i64);
        visit_i128(i128);
        visit_u8(u8);
        visit_u16(u16);
        visit_u32(u32);
        visit_u64(u64);
        visit_u128(u128);
        visit_f32(f32);
        visit_f64(f64);
        visit_char(char);
        visit_str(&str);
        visit_borrowed_str(&'de str);
        visit_string(String);
        visit_bytes(&[u8]);
        visit_borrowed_bytes(&'de [u8]);
        visit_byte_buf(Vec<u8>);
    }

    fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        self.visitor.visit_none()
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        self.visitor.visit_unit()
    }

    fn visit_some<D: Deserializer<'de>>(self, inner: D) -> Result<Self::Value, D::Error> {
        let state = self.state;
        self.visitor.visit_some(Strict { inner, state })
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(self, inner: D) -> Result<Self::Value, D::Error> {
        let state = self.state;
        self.visitor.visit_newtype_struct(Strict { inner, state })
    }

    /// Refused before any element is read.
    fn visit_seq<A: SeqAccess<'de>>(self, inner: A) -> Result<Self::Value, A::Error> {
        self.enter();
        if self.object {
            self.state.refused.set(true);
            return Err(serde::de::Error::invalid_type(
                serde::de::Unexpected::Seq,
                &"a JSON object",
            ));
        }
        let state = self.state;
        self.visitor.visit_seq(Elements {
            inner,
            state,
            start: state.mark(),
            index: 0,
        })
    }

    fn visit_map<A: MapAccess<'de>>(self, inner: A) -> Result<Self::Value, A::Error> {
        self.enter();
        let state = self.state;
        self.visitor.visit_map(Members {
            inner,
            state,
            start: state.mark(),
            key: Key::Absent,
        })
    }

    fn visit_enum<A: EnumAccess<'de>>(self, inner: A) -> Result<Self::Value, A::Error> {
        let state = self.state;
        self.visitor.visit_enum(Strict { inner, state })
    }
}

/// A value read through [`Strict`].
impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Strict<'_, S> {
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, inner: D) -> Result<Self::Value, D::Error> {
        let state = self.state;
        self.inner.deserialize(Strict { inner, state })
    }
}
