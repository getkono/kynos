//! A `flatten`ed field is described as its own members, and a form carries
//! each field as parts under the field's own name, so `MultipartForm` refuses
//! it. The control is `pass/multipart_members_declared_in_place_of_flatten`.

struct Meta {
    title: String,
}

#[derive(kynos::MultipartForm)]
struct Upload {
    #[serde(flatten)]
    meta: Meta,
}

fn main() {}
