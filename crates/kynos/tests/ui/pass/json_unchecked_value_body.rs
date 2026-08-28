fn describes_responses<T: kynos::response::Responses>() {}

fn main() {
    describes_responses::<
        kynos::extract::body::json::Json<
            kynos::schema::unchecked::Unchecked<serde_json::Value>,
        >,
    >();
}
