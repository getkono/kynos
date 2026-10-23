use kynos::{
    extract::{FromRequest, body::text_as::TextAs, describe::RequestContent},
    http::media::MediaType,
    response::{IntoResponse, Responses, negotiate::representation::Representation},
};

struct Csv;

impl MediaType for Csv {
    const MEDIA_TYPE: &'static str = "text/csv";
}

fn from_request<C, T: FromRequest<C> + RequestContent>() {}
fn into_response<T: IntoResponse + Responses + Representation>() {}

fn main() {
    from_request::<(), TextAs<Csv>>();
    into_response::<TextAs<Csv>>();
}
