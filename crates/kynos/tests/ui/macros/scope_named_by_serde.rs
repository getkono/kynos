//! `#[scope(...)]` names an enum serde does not: on one serde writes or
//! reads, derived or written by hand, it would be a second name for a scope.
//! A generic enum is refused, the probe resolving only on a concrete type.
//! The control is `pass/scope_named_without_serde`.

#[derive(Clone, Copy, serde::Serialize, kynos::Scope)]
#[scope(rename_all = "snake_case")]
enum Written {
    ReadReports,
}

#[derive(Clone, Copy, serde::Deserialize, kynos::Scope)]
enum Read {
    #[scope(rename = "reports:read")]
    ReadReports,
}

#[derive(Clone, Copy, kynos::Scope)]
#[scope(rename_all = "snake_case")]
enum HandWritten {
    ReadReports,
}

impl serde::Serialize for HandWritten {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("read_reports")
    }
}

#[derive(Clone, Copy, kynos::Scope)]
enum Generic<const N: usize> {
    Admin,
}

fn main() {}
