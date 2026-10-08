use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
};

use kynos_openapi::{
    Schema as OpenApiSchema, SchemaObject,
    model::schema::types::{SchemaType, TypeSet},
};

use crate::schema::{
    MapKey, ParamValue, Schema,
    flatten::{ClosedFlatten, Flatten},
    registry::Registry,
};

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

fn param_value<T: ParamValue>() {}

/// The `ParamValue` promise for one value: its schema names `format`, its
/// `Display` writes `text`, which is that format's form, and its `FromStr`
/// reads `text` back to the same value.
///
/// A `ParamValue` impl is a claim about the wire form a witness cannot see, so
/// it is worth only as much as a test that produces the text.
fn carries_as<T>(value: &T, text: &str, format: Option<&str>)
where
    T: ParamValue + PartialEq + std::fmt::Debug,
{
    assert_eq!(object_of::<T>().format.as_deref(), format);
    assert_eq!(value.to_string(), text);
    assert_eq!(text.parse::<T>().ok().as_ref(), Some(value));
}

fn flattenable<T: Flatten>() {}

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

#[cfg(feature = "uuid")]
#[test]
fn a_uuid_writes_its_format_and_reads_it_back() {
    let value = uuid::Uuid::from_u128(0x67e5_5044_10b1_426f_9247_bb68_0e5f_e0c8);
    carries_as(&value, "67e55044-10b1-426f-9247-bb680e5fe0c8", Some("uuid"));
}

#[cfg(feature = "time-chrono")]
mod chrono_backend {
    use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};

    use super::{carries_as, object_of, schema_of};

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

    #[test]
    fn a_date_and_a_time_write_their_formats_and_read_them_back() {
        let day = NaiveDate::from_ymd_opt(2026, 3, 15).expect("a representable date");
        carries_as(&day, "2026-03-15", Some("date"));
        let clock = NaiveTime::from_hms_opt(12, 30, 0).expect("a representable time");
        carries_as(&clock, "12:30:00", Some("time-local"));
    }

    /// Why the date-times are not parameter values: `Display` writes a space
    /// where RFC 3339 has a `T`, and `Utc` appends a zone name, so each writes
    /// text its own format refuses. The civil one cannot even read its own.
    #[test]
    fn a_date_time_displays_other_than_its_format() {
        let civil = NaiveDate::from_ymd_opt(2026, 3, 15)
            .and_then(|date| date.and_hms_opt(12, 30, 0))
            .expect("a representable civil date and time");
        assert_eq!(civil.to_string(), "2026-03-15 12:30:00");
        civil
            .to_string()
            .parse::<NaiveDateTime>()
            .expect_err("a civil date-time does not read its own display");

        let utc = DateTime::<Utc>::from_timestamp(0, 0).expect("the epoch is representable");
        assert_eq!(utc.to_string(), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc.fixed_offset().to_string(), "1970-01-01 00:00:00 +00:00");
    }
}

#[cfg(feature = "time-jiff")]
mod jiff_backend {
    use jiff::{
        SignedDuration, Span, Timestamp, Zoned,
        civil::{self, date},
    };

    use super::{carries_as, object_of};

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

    #[test]
    fn each_type_writes_its_format_and_reads_it_back() {
        carries_as(&date(2026, 3, 15), "2026-03-15", Some("date"));
        carries_as(&civil::time(12, 30, 0, 0), "12:30:00", Some("time-local"));
        carries_as(
            &date(2026, 3, 15).at(12, 30, 0, 0),
            "2026-03-15T12:30:00",
            Some("date-time-local"),
        );
        carries_as(
            &Timestamp::UNIX_EPOCH,
            "1970-01-01T00:00:00Z",
            Some("date-time"),
        );
        let zoned: Zoned = date(2024, 6, 19)
            .at(15, 22, 0, 0)
            .in_tz("America/New_York")
            .expect("a bundled zone resolves");
        carries_as(
            &zoned,
            "2024-06-19T15:22:00-04:00[America/New_York]",
            Some("date-time-zoned"),
        );
        carries_as(
            &SignedDuration::from_secs(5_400),
            "PT1H30M",
            Some("duration"),
        );
    }

    /// `Span` compares only field by field, so it cannot go through
    /// `carries_as`.
    #[test]
    fn a_span_writes_its_format_and_reads_it_back() {
        let span = Span::new().hours(1).minutes(30);
        assert_eq!(span.to_string(), "PT1H30M");
        let read: Span = "PT1H30M".parse().expect("an ISO 8601 duration parses");
        assert_eq!(read.fieldwise(), span.fieldwise());
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

    #[cfg(feature = "decimal-rust")]
    #[test]
    fn a_fixed_decimal_writes_its_format_and_reads_it_back() {
        let value = "-1.2300"
            .parse::<rust_decimal::Decimal>()
            .expect("a decimal");
        super::carries_as(&value, "-1.2300", Some("decimal"));
    }

    /// Why `BigDecimal` is not a parameter value: its `Display` writes an
    /// exponent for a value far below one, or for one read from exponent
    /// notation, and the fixed-point `decimal` format refuses both.
    #[cfg(feature = "decimal-big")]
    #[test]
    fn an_arbitrary_decimal_displays_other_than_its_format() {
        let display = |text: &str| {
            text.parse::<bigdecimal::BigDecimal>()
                .expect("an arbitrary-precision decimal")
                .to_string()
        };
        assert_eq!(display("0.0000000000000000001"), "1E-19");
        assert_eq!(display("1e30"), "1e+30");
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
fn each_address_writes_its_format_and_reads_it_back() {
    carries_as(&Ipv4Addr::new(192, 0, 2, 1), "192.0.2.1", Some("ipv4"));
    let v6 = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1);
    carries_as(&v6, "2001:db8::1", Some("ipv6"));
    // Either format, through its `anyOf`.
    carries_as(&IpAddr::V6(v6), "2001:db8::1", None);
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

/// The scalars a parameter field may be. Losing one is breaking, so each one is
/// named here.
#[test]
fn every_shipped_scalar_is_a_parameter_value() {
    param_value::<bool>();
    param_value::<char>();
    param_value::<String>();
    param_value::<i8>();
    param_value::<i16>();
    param_value::<i32>();
    param_value::<i64>();
    param_value::<u8>();
    param_value::<u16>();
    param_value::<u32>();
    param_value::<u64>();
    param_value::<f32>();
    param_value::<f64>();

    param_value::<Ipv4Addr>();
    param_value::<Ipv6Addr>();
    param_value::<IpAddr>();

    #[cfg(feature = "uuid")]
    param_value::<uuid::Uuid>();

    #[cfg(feature = "time-chrono")]
    {
        param_value::<chrono::NaiveDate>();
        param_value::<chrono::NaiveTime>();
    }

    #[cfg(feature = "time-jiff")]
    {
        param_value::<jiff::civil::Date>();
        param_value::<jiff::civil::Time>();
        param_value::<jiff::civil::DateTime>();
        param_value::<jiff::Timestamp>();
        param_value::<jiff::Zoned>();
        param_value::<jiff::Span>();
        param_value::<jiff::SignedDuration>();
    }

    #[cfg(feature = "decimal-rust")]
    param_value::<rust_decimal::Decimal>();
}

#[test]
fn each_standard_scalar_writes_its_format_and_reads_it_back() {
    carries_as(&true, "true", None);
    carries_as(&'é', "é", Some("char"));
    carries_as(&"a b".to_owned(), "a b", None);
    carries_as(&i8::MIN, "-128", Some("int8"));
    carries_as(&i16::MIN, "-32768", Some("int16"));
    carries_as(&i32::MIN, "-2147483648", Some("int32"));
    carries_as(&i64::MIN, "-9223372036854775808", Some("int64"));
    carries_as(&u8::MAX, "255", Some("uint8"));
    carries_as(&u16::MAX, "65535", Some("uint16"));
    carries_as(&u32::MAX, "4294967295", Some("uint32"));
    carries_as(&u64::MAX, "18446744073709551615", Some("uint64"));
    // No exponent even far from one, which a JSON number would also admit.
    carries_as(
        &1.0e30_f32,
        "1000000000000000000000000000000",
        Some("float"),
    );
    carries_as(&-0.5_f64, "-0.5", Some("double"));
}

/// One exception the `ParamValue` docs state: a non-finite float writes text no
/// `number` schema admits, and reads it back.
#[test]
fn a_non_finite_float_writes_what_no_number_admits() {
    assert_eq!(f64::NAN.to_string(), "NaN");
    assert_eq!(f64::INFINITY.to_string(), "inf");
    assert_eq!(f32::NEG_INFINITY.to_string(), "-inf");
    assert_eq!("inf".parse::<f64>(), Ok(f64::INFINITY));
    assert!("NaN".parse::<f32>().is_ok_and(f32::is_nan));
}

/// The chrono exception the `ParamValue` docs state: a year outside 0000–9999
/// takes a sign and, past 9999, a fifth digit, neither of which RFC 3339's
/// `date` admits. serde writes the same text, and `FromStr` reads it back.
#[cfg(feature = "time-chrono")]
#[test]
fn a_chrono_date_outside_four_digit_years_writes_what_no_date_admits() {
    use chrono::NaiveDate;

    for (year, text) in [(10_000, "+10000-01-01"), (-1, "-0001-01-01")] {
        let day = NaiveDate::from_ymd_opt(year, 1, 1).expect("a representable date");
        assert_eq!(day.to_string(), text);
        assert_eq!(
            serde_json::to_value(day).expect("a date serializes"),
            serde_json::Value::String(text.to_owned())
        );
        assert_eq!(text.parse::<NaiveDate>(), Ok(day));
    }
}

/// Two jiff exceptions the `ParamValue` docs state: a year before 0 takes a
/// sign and six digits, and a negative duration a leading `-`, neither of which
/// RFC 3339 admits. serde writes the same text, and `FromStr` reads it back.
#[cfg(feature = "time-jiff")]
#[test]
fn a_jiff_value_before_zero_writes_what_no_format_admits() {
    use jiff::{SignedDuration, Span, civil};

    let day = civil::date(-1, 1, 1);
    assert_eq!(day.to_string(), "-000001-01-01");
    assert_eq!(
        serde_json::to_value(day).expect("a date serializes"),
        serde_json::Value::String("-000001-01-01".to_owned())
    );
    assert_eq!("-000001-01-01".parse::<civil::Date>().ok(), Some(day));

    let back = SignedDuration::from_hours(-1);
    assert_eq!(back.to_string(), "-PT1H");
    assert_eq!(
        serde_json::to_value(back).expect("a duration serializes"),
        serde_json::Value::String("-PT1H".to_owned())
    );
    assert_eq!("-PT1H".parse::<SignedDuration>().ok(), Some(back));

    let span = Span::new().days(-1);
    assert_eq!(span.to_string(), "-P1D");
    assert_eq!(
        serde_json::to_value(span).expect("a span serializes"),
        serde_json::Value::String("-P1D".to_owned())
    );
    let read: Span = "-P1D".parse().expect("a negative ISO 8601 duration parses");
    assert_eq!(read.fieldwise(), span.fieldwise());
}

/// The other jiff duration exception the `ParamValue` docs state: an ordinary
/// non-negative duration can take an ISO 8601 form RFC 3339's `duration` does
/// not admit, with fractional seconds, a skipped unit, or weeks beside days.
/// serde writes the same text, and `FromStr` reads it back.
#[cfg(feature = "time-jiff")]
#[test]
fn a_jiff_duration_writes_iso_8601_forms_no_rfc_3339_duration_admits() {
    use jiff::{SignedDuration, Span};

    for (duration, text) in [
        (SignedDuration::from_secs(3630), "PT1H30S"),
        (SignedDuration::from_millis(500), "PT0.5S"),
    ] {
        assert_eq!(duration.to_string(), text);
        assert_eq!(
            serde_json::to_value(duration).expect("a duration serializes"),
            serde_json::Value::String(text.to_owned())
        );
        assert_eq!(text.parse::<SignedDuration>().ok(), Some(duration));
    }

    let span = Span::new().weeks(1).days(2);
    assert_eq!(span.to_string(), "P1W2D");
    assert_eq!(
        serde_json::to_value(span).expect("a span serializes"),
        serde_json::Value::String("P1W2D".to_owned())
    );
    let read: Span = "P1W2D".parse().expect("an ISO 8601 duration parses");
    assert_eq!(read.fieldwise(), span.fieldwise());
}

/// A type whose `Schema` is written by hand, saying so.
///
/// `Flatten` is unsealed for exactly this: the derive covers the shapes it
/// emits, and a hand-written description that also names its members has to be
/// able to claim the same thing.
struct Audit;

impl Schema for Audit {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        let mut object = SchemaObject {
            ty: Some(TypeSet::One(SchemaType::Object)),
            ..SchemaObject::default()
        };
        object
            .properties
            .insert("at".to_owned(), OpenApiSchema::of_type(SchemaType::String));
        OpenApiSchema::Object(Box::new(object))
    }
}

impl Flatten for Audit {}

impl ClosedFlatten for Audit {}

/// The bound a flattened field is checked against, and the wrappers that carry
/// it across.
#[test]
fn a_type_that_names_its_members_can_be_flattened() {
    flattenable::<Audit>();
    // `Box<T>` and `Arc<T>` are `T`'s description, so they are `T`'s answer.
    flattenable::<Box<Audit>>();
    flattenable::<Arc<Audit>>();
}

/// The narrower bound a closed object's flattened field is checked against,
/// carried across the same wrappers, since serde reads a `Box<T>` or an
/// `Arc<T>` as its `T`.
#[test]
fn a_type_serde_reads_by_name_can_be_flattened_into_a_closed_object() {
    fn closed_flattenable<T: ClosedFlatten + ?Sized>() {}

    closed_flattenable::<Audit>();
    closed_flattenable::<Box<Audit>>();
    closed_flattenable::<Arc<Audit>>();
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

/// The shapes the composite implementations build, exercised through their
/// helpers directly.
///
/// Through the helpers rather than through `Registry::resolve`, because a
/// resolved schema is a `$ref` for anything with a name: what is under test
/// here is the shape, and the reference would hide it.
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

/// How a container locates a violation inside its members, where
/// `tests/constraints.rs` cannot reach it through a derived type: a member the
/// document cannot address, and the first of two reports at one location.
mod checking {
    use std::collections::{BTreeMap, BTreeSet};

    use crate::{
        __private::constraints::{is_multiple, max_length, minimum, unique_items},
        schema::{
            MapKey, Schema,
            constraints::{Pointer, Violations},
            registry::Registry,
        },
    };

    /// A value whose only bound is that it is never `0`, reported at `at`.
    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct NonZero(u8);

    impl Schema for NonZero {
        fn schema(registry: &mut Registry) -> kynos_openapi::Schema {
            u8::schema(registry)
        }

        fn check_constraints(&self, at: Pointer<'_>, violations: &mut Violations) {
            minimum(&self.0, 1.0, at.member("inner"), violations);
        }
    }

    /// A key that cannot say what member name it is written under.
    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct Opaque;

    impl Schema for Opaque {
        fn schema(registry: &mut Registry) -> kynos_openapi::Schema {
            String::schema(registry)
        }
    }

    impl MapKey for Opaque {}

    fn failures<T: Schema>(value: &T) -> BTreeMap<String, String> {
        let mut violations = Violations::new();
        value.check_constraints(Pointer::root().member("v"), &mut violations);
        violations.into_failures()
    }

    #[test]
    fn a_set_member_is_reported_at_the_set_naming_where_inside_it() {
        let failures = failures(&BTreeSet::from([NonZero(0), NonZero(1)]));
        assert_eq!(
            failures,
            BTreeMap::from([(
                "/v".to_owned(),
                "a member breaks a bound at `/inner`: must be at least 1".to_owned()
            )])
        );
    }

    #[test]
    fn a_map_value_is_reported_under_its_key_or_at_the_map_without_one() {
        let named = failures(&BTreeMap::from([("k".to_owned(), NonZero(0))]));
        assert_eq!(named.keys().collect::<Vec<_>>(), ["/v/k/inner"]);

        let opaque = failures(&BTreeMap::from([(Opaque, NonZero(0))]));
        assert_eq!(opaque.keys().collect::<Vec<_>>(), ["/v"]);
    }

    #[test]
    fn an_absent_option_breaks_no_bound() {
        assert!(failures(&None::<NonZero>).is_empty());
        let mut violations = Violations::new();
        max_length(&None::<String>, 0, Pointer::root(), &mut violations);
        assert!(violations.is_empty());
    }

    #[test]
    fn the_first_report_at_a_location_is_the_one_kept() {
        let mut violations = Violations::new();
        violations.report(Pointer::root(), "first");
        violations.report(Pointer::root(), "second");
        assert_eq!(
            violations.into_failures(),
            BTreeMap::from([(String::new(), "first".to_owned())])
        );
    }

    #[test]
    fn uniqueness_finds_a_repeat_wherever_it_sits() {
        for (items, unique) in [
            (vec![3, 1, 2], true),
            (vec![3, 1, 3], false),
            (vec![1, 2, 3, 4, 1], false),
            (vec![], true),
        ] {
            let mut violations = Violations::new();
            unique_items(&items, Pointer::root(), &mut violations);
            assert_eq!(violations.is_empty(), unique, "{items:?}");
        }
    }

    #[test]
    fn a_multiple_is_an_integral_quotient() {
        assert!(is_multiple(-15.0, 5.0));
        assert!(is_multiple(0.0, 5.0));
        assert!(is_multiple(7.5, 2.5));
        assert!(!is_multiple(12.0, 5.0));
        assert!(!is_multiple(1.0, 3.0));
    }
}
