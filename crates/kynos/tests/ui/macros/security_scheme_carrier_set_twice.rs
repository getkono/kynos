#[derive(kynos::SecurityScheme)]
#[security(bearer, carrier = manual, carrier = optional)]
struct Bearer;

fn main() {}
