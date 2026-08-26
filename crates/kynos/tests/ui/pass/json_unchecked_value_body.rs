fn describes_responses<T: kynos::response::Responses>() {}

fn main() {
    describes_responses::<
        kynos::response::codec::json::Json<
            kynos::schema::unchecked::Unchecked<serde_json::Value>,
        >,
    >();
}
