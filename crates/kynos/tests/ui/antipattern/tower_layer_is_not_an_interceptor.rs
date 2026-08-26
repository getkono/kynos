//! README anti-pattern 1: a `tower::Layer` may change the status, rewrite the
//! body, add headers or refuse the request, and nothing in its type says which.
//! It is therefore not accepted where an `Interceptor` is required. Reaching it
//! at all takes `layer_unchecked`, behind the `unchecked` feature.

fn intercepts<C: Sync + 'static, I: kynos::middleware::Interceptor<C>>() {}

fn main() {
    intercepts::<(), tower::layer::util::Identity>();
}
