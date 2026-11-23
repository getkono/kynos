//! A parameter group carries `#[schema(...)]` bounds, and the problem it
//! answers them with, with no `Schema` derive beside it: the control for
//! `macros/params_schema_attribute_refused`.

const INVALID: kynos::schema::constraints::ConstraintProblem =
    kynos::schema::constraints::ConstraintProblem::of_type("https://errors.example.com/invalid");

#[derive(kynos::PathParams)]
#[schema(constraint_problem = INVALID)]
struct ByPath {
    #[schema(maximum = 3)]
    id: u32,
}

#[derive(kynos::HeaderParams)]
struct ByHeader {
    #[schema(minimum = 1)]
    #[header(rename = "X-Limit")]
    limit: u32,
}

#[derive(kynos::Schema, kynos::QueryParams)]
struct ByQuery {
    #[schema(maximum = 3)]
    limit: u32,
}

fn path<T: kynos::extract::params::path::PathParams>() {}
fn header<T: kynos::extract::params::header::HeaderParams>() {}
fn query<T: kynos::extract::params::query::QueryParams>() {}

fn main() {
    path::<ByPath>();
    header::<ByHeader>();
    query::<ByQuery>();
}
