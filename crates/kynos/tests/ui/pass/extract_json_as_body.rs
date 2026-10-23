use kynos::{
    extract::{FromRequest, body::json_as::JsonAs, describe::RequestContent},
    http::media::MediaType,
    response::{IntoResponse, Responses},
};

struct Order;

impl MediaType for Order {
    const MEDIA_TYPE: &'static str = "application/vnd.acme.order+json";
}

fn from_request<C, T: FromRequest<C> + RequestContent>() {}
fn into_response<T: IntoResponse + Responses>() {}

fn main() {
    from_request::<(), JsonAs<Order, String>>();
    into_response::<JsonAs<Order, String>>();
}
