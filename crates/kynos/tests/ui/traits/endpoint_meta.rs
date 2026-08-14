//! Only a route attribute makes an operation, so a bare type is not one.

struct NotAnOperation;

fn is_operation<T: kynos::router::endpoint::meta::EndpointMeta>() {}

fn main() {
    is_operation::<NotAnOperation>();
}
