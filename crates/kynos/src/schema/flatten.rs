//! How a type composes when `#[serde(flatten)]` makes its members another
//! object's.
//!
//! A flattened field's schema is composed into its parent's `allOf` rather than
//! named, so whether that is sound is a property of the field's type, which the
//! `Schema` derive can only ask the compiler about. Each trait here is one such
//! answer, asserted by the derive per flattened field: [`Flatten`] for a type
//! that names its members, [`ClosedFlatten`] for one serde also reads by name
//! out of a closed object, [`OpenMap`] for a map `#[schema(open)]` hoists, and
//! [`AdmitsAny`] for an open map that constrains no member.

use crate::schema::Schema;

/// A type whose schema names its own members, so it can be flattened into
/// another object.
///
/// `#[serde(flatten)]` makes a field's members the *parent's* members, so the
/// parent composes the field's schema rather than naming it. A schema that
/// constrains every member it does not name — `additionalProperties` on a map —
/// then reaches the members the parent declared itself, and the object ends up
/// refusing the JSON its own type writes. The permissive schema reaches nothing,
/// but names nothing either: the members it contributes stay unevaluated, so an
/// open map beside it refuses them instead.
///
/// The marker is Kynos's own because serde has no type-level surface to read:
/// `Serialize` is one method, `flatten` is an internal flag that never leaves
/// `serde_derive`, and what enforces it is a runtime serializer. Bounding a
/// flattened field by this trait is what turns that into a compile error at the
/// field that wrote it. In an object `#[serde(deny_unknown_fields)]` closes the
/// bound is the narrower [`ClosedFlatten`] as well.
///
/// Derived beside [`Schema`] for the shapes whose description is an object
/// naming its members: a struct with named fields, and an enum whose every
/// `oneOf` branch is such an object. Not for a container carrying
/// `#[schema(open)]` or `#[serde(transparent)]`, an externally tagged enum, whose
/// branches admit only their variant key, or an internally tagged enum with a
/// newtype variant.
/// Implemented for [`Problem`](crate::Problem), whose schema names the
/// registered members and admits every other one. Unsealed, for the reason
/// [`MapKey`](super::MapKey) is — a hand-written [`Schema`] that does the same thing has to be
/// able to say so.
///
/// ```no_run
/// # use kynos::schema::{Schema, flatten::Flatten};
/// # struct Audit;
/// # impl Schema for Audit {
/// #     fn schema(_: &mut kynos::schema::registry::Registry) -> kynos::openapi::Schema {
/// #         todo!()
/// #     }
/// # }
/// // `Audit::schema` returns an object whose `properties` names `created_at`
/// // and `created_by`, and which constrains no other member. Flattening it
/// // therefore adds exactly those two to whatever carries it.
/// impl Flatten for Audit {}
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be flattened into another object",
    label = "does not name its members",
    note = "a flattened field's members become the parent's own, so its schema has to name them; a \
            map names none, so its values would reach the properties the object declared itself",
    note = "give it a named field of its own; on a `HashMap` or `BTreeMap` field, add \
            `#[schema(open)]` beside `#[serde(flatten)]` to say the object really is open, so \
            its values become the object's `unevaluatedProperties`; for arbitrary JSON, flatten \
            an `Unchecked<serde_json::Map<String, Value>>` with `#[schema(open)]`, which leaves \
            the object open; as an internally tagged newtype variant's payload, make it a \
            struct variant holding that flattened open field, which serde writes the same way"
)]
pub trait Flatten: Schema {}

/// A [`Flatten`] type serde reads by name, so it can be flattened into an object
/// `#[serde(deny_unknown_fields)]` closes.
///
/// A parent with a flattened field buffers every entry of the object it reads
/// and hands each flattened field the lot. Under `deny_unknown_fields` it then
/// refuses the first entry no field took, and only a type serde reads through
/// `deserialize_struct` takes one: that reader claims each key it names. An
/// internally tagged enum reads through `deserialize_any`, and a struct holding
/// a flattened field serde reads, or a map, through `deserialize_map`, both of
/// which borrow the entries and leave them all in place. Flattened into a closed
/// object, such a type makes serde refuse every document it writes, while the
/// closed schema accepts them. The derive bounds each flattened field of a
/// closed object by this trait as well as by `Flatten`, so that case is a
/// compile error at the field.
///
/// Derived beside `Flatten` for the two shapes serde reads by name: a struct
/// whose fields include no `#[serde(flatten)]` serde reads and which carries no
/// container `#[serde(tag = "...")]`, a key serde writes and never takes, and
/// an adjacently tagged enum, whose tag and content keys serde names. Carried
/// across `Box<T>` and `Arc<T>`. Not implemented for [`Problem`](crate::Problem): serde never
/// reads one, having no `Deserialize` for it, and its `additionalProperties:
/// true` marks every member evaluated, so the closing keyword of an object
/// flattening it would refuse nothing:
///
/// ```compile_fail
/// fn closed_flattenable<T: kynos::schema::flatten::ClosedFlatten>() {}
///
/// closed_flattenable::<kynos::Problem>();
/// ```
///
/// Unsealed, for the reason `Flatten` is — a hand-written `Flatten` whose
/// `Deserialize` reads through `deserialize_struct` has to be able to say so,
/// and has to before it can be flattened into a closed object.
///
/// ```no_run
/// # use kynos::schema::{
/// #     Schema,
/// #     flatten::{ClosedFlatten, Flatten},
/// # };
/// # struct Audit;
/// # impl Schema for Audit {
/// #     fn schema(_: &mut kynos::schema::registry::Registry) -> kynos::openapi::Schema {
/// #         todo!()
/// #     }
/// # }
/// // `Audit`'s `Deserialize` calls `deserialize_struct` naming `created_at` and
/// // `created_by`, the two members its schema names, so it takes both keys out
/// // of a closed object that flattens it.
/// impl Flatten for Audit {}
/// impl ClosedFlatten for Audit {}
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be flattened into an object `#[serde(deny_unknown_fields)]` closes",
    label = "not known to be read by name",
    note = "a closed object refuses every key no flattened field takes, and serde takes a key \
            only for a type it reads by name; it lends an internally tagged enum, a struct \
            holding a `#[serde(flatten)]` field of its own, or a map every key without taking \
            any, and never takes a struct's own `#[serde(tag)]`, so the object would refuse \
            every document such a type writes; one serde never reads, such as `Problem`, leaves \
            the closing keyword nothing to refuse",
    note = "flatten a type serde reads by name instead, either a struct with neither a \
            flattened field nor a `#[serde(tag)]` of its own, or an adjacently tagged enum, \
            `#[serde(tag = \"...\", content = \"...\")]`; move the flattened type's members \
            into the object; or drop `deny_unknown_fields`"
)]
pub trait ClosedFlatten: Flatten {}

/// A map whose schema is written out in place, so `#[schema(open)]` can hoist
/// its values onto the object it is flattened into.
///
/// `#[schema(open)]` moves a flattened map's `additionalProperties` to the
/// parent's `unevaluatedProperties`, which needs the map's own schema object in
/// hand. A type described through a `$ref` leaves nothing to move, and the
/// `additionalProperties` of the schema it refers to would reach the properties
/// the parent declared itself. The derive bounds every open field by this
/// trait, so that case is a compile error at the field.
///
/// Implemented for [`HashMap`](std::collections::HashMap) and
/// [`BTreeMap`](std::collections::BTreeMap), and carried across `Box<T>` and
/// `Arc<T>`. Also for [`Unchecked`](super::unchecked::Unchecked) over a type that
/// implements `OpenMap` itself, or over a `serde_json::Map`, which has no
/// `additionalProperties` to hoist and so leaves the object open — the route
/// for arbitrary JSON beside an object's own members, and an [`AdmitsAny`].
/// Unsealed, for the reason [`MapKey`](super::MapKey) is — a hand-written [`Schema`] that
/// claims no component name and describes an object by `additionalProperties`
/// alone has to be able to say so.
///
/// A key constraint does not survive the hoist. Inside the `allOf` branch
/// `propertyNames` would name the parent's own properties too, so it is dropped:
/// a map keyed by a [`MapKey`](super::MapKey) with [`key_constraints`](super::MapKey::key_constraints)
/// is described more weakly than its type, rather than contradicting it.
///
/// ```no_run
/// # use kynos::schema::{Schema, flatten::OpenMap};
/// # struct Headers;
/// # impl Schema for Headers {
/// #     fn schema(_: &mut kynos::schema::registry::Registry) -> kynos::openapi::Schema {
/// #         todo!()
/// #     }
/// # }
/// // `Headers::schema` returns `{"type": "object", "additionalProperties": ...}`
/// // and `Headers::name` is the default `None`, so the description of a
/// // flattened `Headers` is that object itself.
/// impl OpenMap for Headers {}
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be flattened with `#[schema(open)]`",
    label = "not a map described in place",
    note = "`#[schema(open)]` moves a map's value schema onto the object carrying it, which needs \
            the map's own schema rather than a `$ref` to one",
    note = "flatten the `HashMap` or `BTreeMap` field itself; for arbitrary JSON, flatten an \
            `Unchecked<serde_json::Map<String, Value>>`, not an `Unchecked<Value>`, since serde \
            flattens only a map; or drop `#[schema(open)]` from a type that names its members"
)]
pub trait OpenMap: Schema {}

/// An [`OpenMap`] whose schema constrains no member, so flattening it leaves the
/// object it is flattened into open to every member.
///
/// `#[schema(open)]` hoists an open map's `additionalProperties` onto the object
/// as `unevaluatedProperties`, which constrains every member the object's
/// schema does not name. A named field serde writes and never reads,
/// `#[serde(skip_deserializing)]` alone, is such a member: the schema describes
/// what serde reads, so it leaves the field out, and the hoisted keyword then
/// refuses what serde writes of it. The derive bounds an open field by this
/// trait wherever such a field sits beside it, so that case is a compile error
/// at the open field, and an open field whose hoisted schema refuses nothing is
/// accepted there.
///
/// Implemented for [`Unchecked`](super::unchecked::Unchecked) wherever it is an
/// `OpenMap`, since its schema carries no `additionalProperties`, for a
/// `HashMap` or `BTreeMap` whose values are `Unchecked`, and carried across
/// `Box<T>` and `Arc<T>`:
///
/// ```
/// use std::{collections::{BTreeMap, HashMap}, sync::Arc};
///
/// use kynos::schema::unchecked::Unchecked;
///
/// fn admits_any<T: kynos::schema::flatten::AdmitsAny>() {}
///
/// admits_any::<Unchecked<serde_json::Map<String, serde_json::Value>>>();
/// admits_any::<Box<Unchecked<serde_json::Map<String, serde_json::Value>>>>();
/// admits_any::<Arc<Unchecked<BTreeMap<String, u64>>>>();
/// admits_any::<HashMap<String, Unchecked<serde_json::Value>>>();
/// admits_any::<BTreeMap<String, Unchecked<Vec<u64>>>>();
/// ```
///
/// A map hoists its value schema, so one whose values are typed is not one:
///
/// ```compile_fail
/// fn admits_any<T: kynos::schema::flatten::AdmitsAny>() {}
///
/// admits_any::<std::collections::BTreeMap<String, u64>>();
/// ```
///
/// Unsealed, for the reason [`MapKey`](super::MapKey) is — a hand-written open map whose schema
/// has no `additionalProperties` has to be able to say so.
///
/// ```no_run
/// # use kynos::schema::{
/// #     Schema,
/// #     flatten::{AdmitsAny, OpenMap},
/// # };
/// # struct Passthrough;
/// # impl Schema for Passthrough {
/// #     fn schema(_: &mut kynos::schema::registry::Registry) -> kynos::openapi::Schema {
/// #         todo!()
/// #     }
/// # }
/// // Where `Passthrough::schema` returns `{"type": "object"}`, with no
/// // `additionalProperties`, hoisting it gives the object no
/// // `unevaluatedProperties` at all.
/// impl OpenMap for Passthrough {}
/// impl AdmitsAny for Passthrough {}
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be flattened open beside a field serde writes and never reads",
    label = "not an open map that constrains no member",
    note = "a `skip_deserializing` field is left out of the schema, since serde never reads it, \
            but serde still writes it, so an open field beside it has to leave the object open to \
            members the schema does not name; a map's value schema becomes the object's \
            `unevaluatedProperties` and refuses the field unless its values are `Unchecked`",
    note = "use `#[serde(skip)]` to leave the field out both ways, drop `skip_deserializing` so \
            the schema names it, or flatten an `Unchecked<serde_json::Map<String, Value>>` or a \
            map whose values are `Unchecked`, which leave the object open"
)]
pub trait AdmitsAny: OpenMap {}
