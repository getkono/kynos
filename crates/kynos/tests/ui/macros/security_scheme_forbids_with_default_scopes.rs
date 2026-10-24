#[derive(kynos::SecurityScheme)]
#[security(bearer, scopes("audit"), forbids = false)]
struct Bearer;

fn main() {}
