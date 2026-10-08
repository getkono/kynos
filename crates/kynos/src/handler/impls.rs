//! [`Handler`] for functions of up to sixteen extractors, after an optional
//! guard.
//!
//! Four implementations per arity: the last argument consumes the body or every
//! argument reads only the head, each with and without a guard in front. They
//! are told apart by the markers leading the argument tuple, because a function
//! of `n` arguments matches several shapes and coherence has no other way to
//! see the difference. There is no shape with two guards, which is what makes a
//! second one a compile error.

use std::future::Future;

use crate::{
    extract::{FromRequest, FromRequestParts, describe::Describe},
    handler::{Guarded, Handler, ViaParts, ViaRequest},
    http::{Request, Response},
    response::{IntoResponse, Responses},
    router::operation::OperationCx,
    security::Guard,
};

/// Runs the guard, short-circuiting into its rejection's response.
macro_rules! guard {
    ($ty:ident, $parts:expr, $context:expr) => {
        match <$ty as Guard<C>>::guard(&$parts, $context).await {
            Ok(value) => value,
            Err(rejection) => return rejection.into_response(),
        }
    };
}

/// Merges the guard's description and its rejection's responses.
macro_rules! describe_guard {
    ($ty:ident, $operation:expr) => {{
        <$ty as Describe>::describe($operation);
        let rejected =
            <<$ty as Guard<C>>::Rejection as Responses>::responses($operation.registry());
        $operation.add_responses(&rejected);
    }};
}

/// Runs one head extractor, short-circuiting into its rejection's response.
macro_rules! extract_parts {
    ($ty:ident, $parts:expr, $context:expr) => {
        match <$ty as FromRequestParts<C>>::from_request_parts(&mut $parts, $context).await {
            Ok(value) => value,
            Err(rejection) => return rejection.into_response(),
        }
    };
}

/// Merges one head argument's description and its rejection's responses.
macro_rules! describe_parts {
    ($ty:ident, $operation:expr) => {{
        <$ty as Describe>::describe($operation);
        let rejected = <<$ty as FromRequestParts<C>>::Rejection as Responses>::responses(
            $operation.registry(),
        );
        $operation.add_responses(&rejected);
    }};
}

/// Emits both implementations for one arity.
macro_rules! impl_handler {
    ( $($head:ident),* ; $last:ident ) => {
        // --- the last argument consumes the body ---------------------------
        impl<C, F, Fut, Res, $($head,)* $last>
            Handler<C, (ViaRequest, $($head,)* $last)> for F
        where
            F: FnOnce($($head,)* $last) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = Res> + Send,
            Res: IntoResponse + Responses,
            C: Send + Sync + 'static,
            $( $head: FromRequestParts<C> + Describe, )*
            $last: FromRequest<C> + Describe,
        {
            #[allow(non_snake_case, unused_mut, unused_variables)]
            async fn call(self, request: Request, context: &C) -> Response {
                let (mut parts, body) = request.into_parts();
                $( let $head = extract_parts!($head, parts, context); )*
                let request = Request::from_parts(parts, body);
                let $last = match <$last as FromRequest<C>>::from_request(request, context).await {
                    Ok(value) => value,
                    Err(rejection) => return rejection.into_response(),
                };
                self($($head,)* $last).await.into_response()
            }

            fn describe(operation: &mut OperationCx<'_>) {
                $( describe_parts!($head, operation); )*
                <$last as Describe>::describe(operation);
                let rejected =
                    <<$last as FromRequest<C>>::Rejection as Responses>::responses(
                        operation.registry(),
                    );
                operation.add_responses(&rejected);
                let returned = <Res as Responses>::responses(operation.registry());
                operation.add_responses(&returned);
            }
        }

        // --- every argument reads the head only ----------------------------
        impl<C, F, Fut, Res, $($head,)* $last>
            Handler<C, (ViaParts, $($head,)* $last)> for F
        where
            F: FnOnce($($head,)* $last) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = Res> + Send,
            Res: IntoResponse + Responses,
            C: Send + Sync + 'static,
            $( $head: FromRequestParts<C> + Describe, )*
            $last: FromRequestParts<C> + Describe,
        {
            #[allow(non_snake_case, unused_mut, unused_variables)]
            async fn call(self, request: Request, context: &C) -> Response {
                let (mut parts, _body) = request.into_parts();
                $( let $head = extract_parts!($head, parts, context); )*
                let $last = extract_parts!($last, parts, context);
                self($($head,)* $last).await.into_response()
            }

            fn describe(operation: &mut OperationCx<'_>) {
                $( describe_parts!($head, operation); )*
                describe_parts!($last, operation);
                let returned = <Res as Responses>::responses(operation.registry());
                operation.add_responses(&returned);
            }
        }

        // --- a guard, then the last argument consumes the body -------------
        impl<C, F, Fut, Res, G, $($head,)* $last>
            Handler<C, (Guarded, ViaRequest, G, $($head,)* $last)> for F
        where
            F: FnOnce(G, $($head,)* $last) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = Res> + Send,
            Res: IntoResponse + Responses,
            C: Send + Sync + 'static,
            G: Guard<C>,
            $( $head: FromRequestParts<C> + Describe, )*
            $last: FromRequest<C> + Describe,
        {
            #[allow(non_snake_case, unused_mut, unused_variables)]
            async fn call(self, request: Request, context: &C) -> Response {
                let (mut parts, body) = request.into_parts();
                let G = guard!(G, parts, context);
                $( let $head = extract_parts!($head, parts, context); )*
                let request = Request::from_parts(parts, body);
                let $last = match <$last as FromRequest<C>>::from_request(request, context).await {
                    Ok(value) => value,
                    Err(rejection) => return rejection.into_response(),
                };
                self(G, $($head,)* $last).await.into_response()
            }

            fn describe(operation: &mut OperationCx<'_>) {
                describe_guard!(G, operation);
                $( describe_parts!($head, operation); )*
                <$last as Describe>::describe(operation);
                let rejected =
                    <<$last as FromRequest<C>>::Rejection as Responses>::responses(
                        operation.registry(),
                    );
                operation.add_responses(&rejected);
                let returned = <Res as Responses>::responses(operation.registry());
                operation.add_responses(&returned);
            }
        }

        // --- a guard, then every argument reads the head only --------------
        impl<C, F, Fut, Res, G, $($head,)* $last>
            Handler<C, (Guarded, ViaParts, G, $($head,)* $last)> for F
        where
            F: FnOnce(G, $($head,)* $last) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = Res> + Send,
            Res: IntoResponse + Responses,
            C: Send + Sync + 'static,
            G: Guard<C>,
            $( $head: FromRequestParts<C> + Describe, )*
            $last: FromRequestParts<C> + Describe,
        {
            #[allow(non_snake_case, unused_mut, unused_variables)]
            async fn call(self, request: Request, context: &C) -> Response {
                let (mut parts, _body) = request.into_parts();
                let G = guard!(G, parts, context);
                $( let $head = extract_parts!($head, parts, context); )*
                let $last = extract_parts!($last, parts, context);
                self(G, $($head,)* $last).await.into_response()
            }

            fn describe(operation: &mut OperationCx<'_>) {
                describe_guard!(G, operation);
                $( describe_parts!($head, operation); )*
                describe_parts!($last, operation);
                let returned = <Res as Responses>::responses(operation.registry());
                operation.add_responses(&returned);
            }
        }
    };
}

/// A handler that takes nothing needs no marker: there is nothing to tell
/// apart, so `A` is the empty tuple.
impl<C, F, Fut, Res> Handler<C, ()> for F
where
    F: FnOnce() -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Res> + Send,
    Res: IntoResponse + Responses,
    C: Send + Sync + 'static,
{
    async fn call(self, request: Request, context: &C) -> Response {
        let _ = (request, context);
        self().await.into_response()
    }

    fn describe(operation: &mut OperationCx<'_>) {
        let returned = <Res as Responses>::responses(operation.registry());
        operation.add_responses(&returned);
    }
}

/// A handler that takes a guard and nothing else reads the head only, so it
/// needs only the guard's marker.
impl<C, F, Fut, Res, G> Handler<C, (Guarded, G)> for F
where
    F: FnOnce(G) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Res> + Send,
    Res: IntoResponse + Responses,
    C: Send + Sync + 'static,
    G: Guard<C>,
{
    #[allow(non_snake_case)]
    async fn call(self, request: Request, context: &C) -> Response {
        let (parts, _body) = request.into_parts();
        let G = guard!(G, parts, context);
        self(G).await.into_response()
    }

    fn describe(operation: &mut OperationCx<'_>) {
        describe_guard!(G, operation);
        let returned = <Res as Responses>::responses(operation.registry());
        operation.add_responses(&returned);
    }
}

impl_handler!(; T1);
impl_handler!(T1; T2);
impl_handler!(T1, T2; T3);
impl_handler!(T1, T2, T3; T4);
impl_handler!(T1, T2, T3, T4; T5);
impl_handler!(T1, T2, T3, T4, T5; T6);
impl_handler!(T1, T2, T3, T4, T5, T6; T7);
impl_handler!(T1, T2, T3, T4, T5, T6, T7; T8);
impl_handler!(T1, T2, T3, T4, T5, T6, T7, T8; T9);
impl_handler!(T1, T2, T3, T4, T5, T6, T7, T8, T9; T10);
impl_handler!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10; T11);
impl_handler!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11; T12);
impl_handler!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12; T13);
impl_handler!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13; T14);
impl_handler!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14; T15);
impl_handler!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15; T16);
