use kynos::extract::params::path::Path;

#[kynos::get("/teams/{team}/members/{member}")]
async fn get_member(Path(member): Path<u64>) {
    let _ = member;
}

fn main() {}
