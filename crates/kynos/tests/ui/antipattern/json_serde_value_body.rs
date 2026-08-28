//! README anti-pattern 6: `Json<serde_json::Value>` has no schema.

fn describes_responses<T: kynos::response::Responses>() {}

fn main() {
    describes_responses::<kynos::extract::body::json::Json<serde_json::Value>>();
}
