use kynos::{
    http::{Request, Response},
    middleware::{Interceptor, Next, contribution::OperationContribution},
    router::operation::Route,
};

struct Timing;

impl<C: Sync + 'static> Interceptor<C> for Timing {
    fn contribution(&self, route: Route<'_>) -> OperationContribution {
        let _ = route;
        OperationContribution::none()
    }

    async fn intercept(&self, request: Request, context: &C, next: Next<'_, C>) -> Response {
        let _ = (request, context, next);
        todo!()
    }
}

fn intercepts<C: Sync + 'static, I: Interceptor<C>>() {}

fn main() {
    intercepts::<(), Timing>();
}
