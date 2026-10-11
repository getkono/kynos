#[derive(kynos::SecurityScheme)]
#[security(bearer, carrier = automatic)]
struct Bearer;

fn main() {}
