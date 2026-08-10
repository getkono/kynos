#[derive(kynos::SecurityScheme)]
#[security(bearer)]
struct Bearer {
    token: String,
}

fn main() {}
