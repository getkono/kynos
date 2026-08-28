//! [`Handler`] for functions of up to sixteen arguments.
//!
//! Two implementations per arity: one where the last argument consumes the
//! body, one where every argument reads only the head. They are told apart by
//! the marker in the first slot of the argument tuple, because a function of
//! `n` arguments matches both shapes and coherence has no other way to see the
//! difference.

use std::future::Future;

use crate::{
    extract::{FromRequest, FromRequestParts, describe::Describe},
    handler::{Handler, ViaParts, ViaRequest},
    http::{Request, Response},
    response::{IntoResponse, Responses},
    router::operation::OperationCx,
};

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
