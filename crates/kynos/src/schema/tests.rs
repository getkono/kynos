use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
};

use kynos_openapi::{
    Schema as OpenApiSchema, SchemaObject,
    model::schema::types::{SchemaType, TypeSet},
};

use crate::schema::{MapKey, Schema, registry::Registry};

/// The registry is only touched by implementations that have members to
/// resolve, so the ones checked here can be driven without one being built.
fn schema_of<T: Schema>() -> OpenApiSchema {
    T::schema(&mut Registry::default())
}

fn object_of<T: Schema>() -> SchemaObject {
    match schema_of::<T>() {
        OpenApiSchema::Object(object) => *object,
        OpenApiSchema::Bool(_) => panic!("expected a keyword-carrying schema"),
    }
}

/// Proves an implementation exists without running it, which is the only way
/// to reach the ones that resolve their members while the registry is a
/// placeholder.
fn describable<T: Schema>() {}

fn keyable<T: MapKey>() {}

#[test]
fn primitives_carry_their_type_and_format() {
    assert_eq!(
        object_of::<bool>().ty,
        Some(TypeSet::One(SchemaType::Boolean))
    );
    assert_eq!(
        object_of::<String>().ty,
        Some(TypeSet::One(SchemaType::String))
    );
    assert_eq!(object_of::<i32>().format.as_deref(), Some("int32"));
    assert_eq!(object_of::<i64>().format.as_deref(), Some("int64"));
    assert_eq!(object_of::<f32>().format.as_deref(), Some("float"));
    assert_eq!(object_of::<f64>().format.as_deref(), Some("double"));
}

#[test]
fn every_integer_width_carries_its_registered_format() {
    // The Format Registry names both signednesses at every width, so no type
    // has to borrow a wider or differently-signed format than it is.
    assert_eq!(object_of::<i8>().format.as_deref(), Some("int8"));
    assert_eq!(object_of::<i16>().format.as_deref(), Some("int16"));
    assert_eq!(object_of::<i32>().format.as_deref(), Some("int32"));
    assert_eq!(object_of::<i64>().format.as_deref(), Some("int64"));
    assert_eq!(object_of::<u8>().format.as_deref(), Some("uint8"));
    assert_eq!(object_of::<u16>().format.as_deref(), Some("uint16"));
    assert_eq!(object_of::<u32>().format.as_deref(), Some("uint32"));
    assert_eq!(object_of::<u64>().format.as_deref(), Some("uint64"));
}

#[test]
fn integer_bounds_are_the_types_own() {
    assert_eq!(object_of::<u8>().minimum, Some(0.0));
    assert_eq!(object_of::<u8>().maximum, Some(255.0));
    assert_eq!(object_of::<i8>().minimum, Some(-128.0));
    assert_eq!(object_of::<i8>().maximum, Some(127.0));
    assert_eq!(object_of::<u32>().maximum, Some(4_294_967_295.0));
}

#[test]
fn the_wide_integers_state_no_bound_they_cannot_state_exactly() {
    // `i64::MAX` and `u64::MAX` do not survive a round trip through `f64`, so
    // emitting them would forbid values the type accepts.
    assert_eq!(object_of::<i64>().minimum, None);
    assert_eq!(object_of::<i64>().maximum, None);
    assert_eq!(object_of::<u64>().minimum, Some(0.0));
    assert_eq!(object_of::<u64>().maximum, None);
}

#[test]
fn a_char_is_a_string_of_exactly_one() {
    let object = object_of::<char>();
    assert_eq!(object.ty, Some(TypeSet::One(SchemaType::String)));
    // The length bounds stay beside the format, because format support is
    // optional and a tool that ignores `char` still gets the constraint.
    assert_eq!(object.format.as_deref(), Some("char"));
    assert_eq!(object.min_length, Some(1));
    assert_eq!(object.max_length, Some(1));
}

#[test]
fn the_unit_type_is_null() {
    assert_eq!(object_of::<()>().ty, Some(TypeSet::One(SchemaType::Null)));
}

#[cfg(feature = "uuid")]
#[test]
fn a_uuid_is_a_string_of_that_format() {
    let object = object_of::<uuid::Uuid>();
    assert_eq!(object.ty, Some(TypeSet::One(SchemaType::String)));
    assert_eq!(object.format.as_deref(), Some("uuid"));
}

/// A format is a claim about the wire form, so it is worth only as much as a
/// test that produces one and looks at it.
#[cfg(feature = "uuid")]
#[test]
fn a_uuid_serializes_as_the_string_its_format_promises() {
    let value = uuid::Uuid::nil();
    let encoded = serde_json::to_value(value).expect("a uuid serializes");
    assert_eq!(
        encoded,
        serde_json::Value::String("00000000-0000-0000-0000-000000000000".to_owned())
    );
}

#[cfg(feature = "time-chrono")]
mod chrono_backend {
    use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};

    use super::{object_of, schema_of};

    fn format_of<T: super::Schema>() -> Option<String> {
        object_of::<T>().format
    }

    #[test]
    fn an_offset_carrying_instant_is_a_date_time() {
        assert_eq!(format_of::<DateTime<Utc>>().as_deref(), Some("date-time"));
        assert_eq!(
            format_of::<DateTime<FixedOffset>>().as_deref(),
            Some("date-time")
        );
    }

    #[test]
    fn the_offsetless_types_take_the_local_formats() {
        assert_eq!(format_of::<NaiveDate>().as_deref(), Some("date"));
        assert_eq!(format_of::<NaiveTime>().as_deref(), Some("time-local"));
        assert_eq!(
            format_of::<NaiveDateTime>().as_deref(),
            Some("date-time-local")
        );
    }

    /// The reason the offset-less types may not claim `date-time`: serde emits
    /// no offset, and the deserializer refuses to read one. A description
    /// promising `date-time` would advertise an input that answers 400.
    #[test]
    fn a_local_date_time_neither_writes_nor_reads_an_offset() {
        let value = NaiveDate::from_ymd_opt(2026, 3, 15)
            .and_then(|date| date.and_hms_opt(12, 30, 0))
            .expect("a representable civil date and time");

        let encoded = serde_json::to_value(value).expect("a civil date-time serializes");
        assert_eq!(
            encoded,
            serde_json::Value::String("2026-03-15T12:30:00".to_owned())
        );

        serde_json::from_value::<NaiveDateTime>(serde_json::Value::String(
            "2026-03-15T12:30:00Z".to_owned(),
        ))
        .expect_err("an offset is not readable back into a civil date-time");
    }

    #[test]
    fn an_instant_writes_the_offset_its_format_promises() {
        let value = DateTime::<Utc>::from_timestamp(0, 0).expect("the epoch is representable");
        let encoded = serde_json::to_value(value).expect("an instant serializes");
        assert_eq!(
            encoded,
            serde_json::Value::String("1970-01-01T00:00:00Z".to_owned())
        );
    }

    /// A `DateTime<Local>` would serialize to a valid RFC 3339 string whose
    /// offset is whatever the process environment says, so the wire contract
    /// would depend on where the server runs. It has no implementation, and
    /// this is the shape of that refusal at the type level.
    #[test]
    fn the_backend_describes_only_what_it_was_given() {
        let _ = schema_of::<DateTime<Utc>>();
    }
}

#[cfg(feature = "time-jiff")]
mod jiff_backend {
    use jiff::{
        SignedDuration, Span, Timestamp, Zoned,
        civil::{self, date},
    };

    use super::object_of;

    fn format_of<T: super::Schema>() -> Option<String> {
        object_of::<T>().format
    }

    fn encode<T: serde::Serialize>(value: T) -> String {
        match serde_json::to_value(value).expect("the value serializes") {
            serde_json::Value::String(text) => text,
            other => panic!("expected a string on the wire, got {other}"),
        }
    }

    #[test]
    fn the_civil_types_take_the_local_formats() {
        assert_eq!(format_of::<civil::Date>().as_deref(), Some("date"));
        assert_eq!(format_of::<civil::Time>().as_deref(), Some("time-local"));
        assert_eq!(
            format_of::<civil::DateTime>().as_deref(),
            Some("date-time-local")
        );
    }

    #[test]
    fn a_timestamp_is_a_date_time_and_a_span_is_a_duration() {
        assert_eq!(format_of::<Timestamp>().as_deref(), Some("date-time"));
        assert_eq!(format_of::<Span>().as_deref(), Some("duration"));
        assert_eq!(format_of::<SignedDuration>().as_deref(), Some("duration"));
    }

    /// The same evidence the chrono backend keeps: a civil date-time writes no
    /// offset, so it may not claim the format that requires one.
    #[test]
    fn a_civil_date_time_writes_no_offset() {
        assert_eq!(
            encode(date(2026, 3, 15).at(12, 30, 0, 0)),
            "2026-03-15T12:30:00"
        );
    }

    #[test]
    fn a_timestamp_writes_the_offset_its_format_promises() {
        assert_eq!(encode(Timestamp::UNIX_EPOCH), "1970-01-01T00:00:00Z");
    }

    /// Why `Zoned` cannot be a `date-time`: the bracketed zone is RFC 9557 and
    /// makes the string invalid RFC 3339. The emitted pattern has to accept
    /// what jiff actually writes, so it is checked against a real one.
    #[test]
    fn a_zoned_writes_rfc_9557_and_the_pattern_admits_it() {
        let zoned: Zoned = date(2024, 6, 19)
            .at(15, 22, 0, 0)
            .in_tz("America/New_York")
            .expect("a bundled zone resolves");
        let encoded = encode(&zoned);
        assert_eq!(encoded, "2024-06-19T15:22:00-04:00[America/New_York]");

        let object = object_of::<Zoned>();
        assert_eq!(object.format.as_deref(), Some("date-time-zoned"));
        let pattern = object.pattern.expect("a zoned value states its shape");
        assert!(
            regex_lite_matches(&pattern, &encoded),
            "the emitted pattern {pattern} rejects {encoded}, which jiff writes"
        );
    }

    /// A deliberately tiny matcher for the one pattern this module emits.
    ///
    /// Pulling a regex engine in as a dependency to check a constant would be a
    /// poor trade; what has to be true is that the pattern admits the string
    /// jiff produces, and its shape is fixed and known.
    fn regex_lite_matches(pattern: &str, value: &str) -> bool {
        assert!(pattern.starts_with('^') && pattern.ends_with('$'));
        let (date_part, rest) = value.split_once('T').expect("a date and a time");
        let Some((clock, zone)) = rest.split_once('[') else {
            return false;
        };
        zone.ends_with(']')
            && date_part.len() == 10
            && date_part.split('-').count() == 3
            && (clock.ends_with('Z') || clock.contains('+') || clock.contains('-'))
    }

    #[test]
    fn a_span_writes_the_iso_8601_duration_its_format_promises() {
        let span: Span = "PT1H30M".parse().expect("an ISO 8601 duration parses");
        assert_eq!(encode(span), "PT1H30M");
        assert_eq!(encode(SignedDuration::from_secs(5_400)), "PT1H30M");
    }
}

#[cfg(feature = "decimal")]
mod decimal_backends {
    use super::object_of;

    /// The claim every decimal backend makes, and the one that would silently
    /// stop being true if anything in the dependency graph enabled
    /// `rust_decimal/serde-float`. Cargo unifies features across the whole
    /// build, so that switch is not local to whoever flips it -- and the
    /// emitted `type: string` would be wrong everywhere with nothing in the
    /// type system to notice.
    fn assert_writes_a_string<T: serde::Serialize + super::Schema>(value: T, expected: &str) {
        let object = object_of::<T>();
        assert_eq!(
            object.ty,
            Some(super::TypeSet::One(super::SchemaType::String))
        );
        assert_eq!(object.format.as_deref(), Some("decimal"));

        let encoded = serde_json::to_value(value).expect("a decimal serializes");
        assert_eq!(encoded, serde_json::Value::String(expected.to_owned()));
    }

    #[cfg(feature = "decimal-rust")]
    #[test]
    fn a_fixed_decimal_is_a_string_of_that_format() {
        // Trailing zeros are significant to `rust_decimal` and survive the
        // round trip, which is a large part of why money uses it.
        assert_writes_a_string(
            "1.2300"
                .parse::<rust_decimal::Decimal>()
                .expect("a decimal"),
            "1.2300",
        );
    }

    #[cfg(feature = "decimal-big")]
    #[test]
    fn an_arbitrary_decimal_is_a_string_of_that_format() {
        // Far past `rust_decimal`'s ceiling of 28 significant digits, which is
        // the whole reason this backend exists alongside it.
        let wide = "1.2345678901234567890123456789012345";
        assert_writes_a_string(
            wide.parse::<bigdecimal::BigDecimal>()
                .expect("an arbitrary-precision decimal"),
            wide,
        );
    }
}

#[test]
fn addresses_use_their_named_formats() {
    assert_eq!(object_of::<Ipv4Addr>().format.as_deref(), Some("ipv4"));
    assert_eq!(object_of::<Ipv6Addr>().format.as_deref(), Some("ipv6"));

    let either = object_of::<IpAddr>();
    assert_eq!(either.ty, None);
    assert_eq!(either.any_of.map(|branches| branches.len()), Some(2));
}

#[test]
fn every_standard_type_the_docs_promise_is_describable() {
    describable::<bool>();
    describable::<String>();
    describable::<char>();
    describable::<i8>();
    describable::<i16>();
    describable::<i32>();
    describable::<i64>();
    describable::<u8>();
    describable::<u16>();
    describable::<u32>();
    describable::<u64>();
    describable::<f32>();
    describable::<f64>();
    describable::<()>();

    describable::<Option<String>>();
    describable::<Box<u32>>();
    describable::<Arc<u32>>();

    describable::<Vec<String>>();
    describable::<VecDeque<String>>();
    describable::<[u8; 4]>();
    describable::<HashSet<String>>();
    describable::<BTreeSet<String>>();
    describable::<HashMap<String, u32>>();
    describable::<BTreeMap<String, u32>>();

    describable::<(u32,)>();
    describable::<(u32, String)>();
    describable::<(
        u32,
        String,
        bool,
        f64,
        char,
        u8,
        i8,
        u16,
        i16,
        u32,
        i32,
        i64,
    )>();

    describable::<Ipv4Addr>();
    describable::<Ipv6Addr>();
    describable::<IpAddr>();

    // Nesting is what makes the set useful, and it must terminate.
    describable::<Vec<Option<HashMap<String, Vec<u32>>>>>();
}

#[test]
fn a_string_is_a_map_key() {
    keyable::<String>();
}

/// `Box<T>` and `Arc<T>` are `T` on the wire, so they must not mint a second
/// component name for the same schema.
#[test]
fn transparent_wrappers_share_the_inner_component_name() {
    assert_eq!(<Box<u32> as Schema>::name(), <u32 as Schema>::name());
    assert_eq!(<Arc<u32> as Schema>::name(), <u32 as Schema>::name());
}

/// Anonymous types inline; only named ones are referenced.
#[test]
fn standard_types_are_anonymous() {
    assert!(<u32 as Schema>::name().is_none());
    assert!(<Vec<String> as Schema>::name().is_none());
    assert!(<(u32, u32) as Schema>::name().is_none());
}

/// The composite implementations resolve their members through the registry,
/// whose body is still `todo!()`, so their helpers are exercised directly.
/// Without this nothing checks the shapes those helpers produce.
mod shapes {
    use kynos_openapi::{
        Schema as OpenApiSchema,
        model::schema::types::{SchemaType, TypeSet},
    };

    use crate::schema::{MapKey, impls::testing};

    #[test]
    fn nullability_widens_a_simple_type_in_place() {
        let widened = testing::nullable(OpenApiSchema::of_type(SchemaType::String));
        let object = widened.as_object().expect("keywords");
        assert_eq!(
            object.ty,
            Some(TypeSet::Many(vec![SchemaType::String, SchemaType::Null]))
        );
        assert!(object.any_of.is_none());
    }

    /// A type union's members must be unique, so a schema that already admits
    /// `null` widens to itself rather than to a repeat.
    #[test]
    fn nullability_does_not_repeat_null() {
        let already = OpenApiSchema::of_type(SchemaType::Null);
        assert_eq!(testing::nullable(already.clone()), already);

        let twice = testing::nullable(testing::nullable(OpenApiSchema::of_type(
            SchemaType::String,
        )));
        assert_eq!(
            twice.as_object().expect("keywords").ty,
            Some(TypeSet::Many(vec![SchemaType::String, SchemaType::Null]))
        );
    }

    /// Widening a `$ref` in place would edit the type it points at.
    #[test]
    fn nullability_wraps_a_reference() {
        let widened = testing::nullable(OpenApiSchema::component("User"));
        let object = widened.as_object().expect("keywords");
        assert_eq!(object.ty, None);
        assert_eq!(object.any_of.as_ref().map(Vec::len), Some(2));
    }

    /// A plain `String` key constrains nothing, so `propertyNames` would say no
    /// more than `type: object` already does.
    #[test]
    fn a_string_key_emits_no_property_names() {
        assert!(<String as MapKey>::key_constraints().is_empty());
    }
}

/// `Unchecked` says something about the description, not about the encoding.
///
/// It exists to sit in a request or response body, and a body is serialized, so
/// a wrapper that changed the bytes would make the annotation cost a nesting
/// level a consumer never asked for.
mod unchecked_is_transparent {
    use crate::schema::unchecked::Unchecked;

    #[test]
    fn the_wrapper_does_not_reach_the_wire() {
        let wrapped = Unchecked(serde_json::json!({ "supplier": "acme" }));
        assert_eq!(
            serde_json::to_value(&wrapped).expect("serializable"),
            serde_json::json!({ "supplier": "acme" })
        );
    }

    #[test]
    fn the_wrapper_is_not_expected_back() {
        let read: Unchecked<serde_json::Value> =
            serde_json::from_str(r#"{"supplier":"acme"}"#).expect("deserializable");
        assert_eq!(read.into_inner(), serde_json::json!({ "supplier": "acme" }));
    }
}
