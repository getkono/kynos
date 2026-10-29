//! [`Handler`] for functions of up to sixteen extractors, after an optional
//! guard.
//!
//! Four implementations per arity (body or head-only, each with or without a
//! guard), kept disjoint by the markers leading the argument tuple.

use std::future::Future;

use crate::{
    extract::{FromRequest, FromRequestParts, describe::Describe},
    handler::{Guarded, Handler, ViaParts, ViaRequest},
    http::{Parts, Response, body::Body},
    response::{IntoResponse, Responses},
    router::operation::OperationCx,
    security::Guard,
};

/// Awaits a guard or extractor, short-circuiting into its rejection's response.
macro_rules! or_respond {
    ($future:expr) => {
        match $future.await {
            Ok(value) => value,
            Err(rejection) => return rejection.into_response(),
        }
    };
}

/// Merges one argument's description and the responses of the rejection it
/// raises as `$extractor`.
macro_rules! describe_argument {
    ($ty:ident as $extractor:ident, $operation:expr) => {{
        <$ty as Describe>::describe($operation);
        let rejected =
            <<$ty as $extractor<C>>::Rejection as Responses>::responses($operation.registry());
        $operation.add_responses(&rejected);
    }};
}

/// Emits the four implementations for one arity: with or without a guard, and
/// with the last argument consuming the body or reading the head only.
///
/// A guard's description declares its refusals itself, so its rejection's
/// responses are not merged: only the guard knows its schemes, and its
/// rejection's `Responses` would add a 403 a scheme that never forbids leaves
/// out.
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
            #[allow(non_snake_case, unused_variables)]
            async fn call(self, parts: &mut Parts, body: Body, context: &C) -> Response {
                $( let $head = or_respond!($head::from_request_parts(parts, context)); )*
                let $last = or_respond!($last::from_request(parts, body, context));
                self($($head,)* $last).await.into_response()
            }

            fn describe(operation: &mut OperationCx<'_>) {
                $( describe_argument!($head as FromRequestParts, operation); )*
                describe_argument!($last as FromRequest, operation);
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
            #[allow(non_snake_case, unused_variables)]
            async fn call(self, parts: &mut Parts, _body: Body, context: &C) -> Response {
                $( let $head = or_respond!($head::from_request_parts(parts, context)); )*
                let $last = or_respond!($last::from_request_parts(parts, context));
                self($($head,)* $last).await.into_response()
            }

            fn describe(operation: &mut OperationCx<'_>) {
                $( describe_argument!($head as FromRequestParts, operation); )*
                describe_argument!($last as FromRequestParts, operation);
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
            #[allow(non_snake_case, unused_variables)]
            async fn call(self, parts: &mut Parts, body: Body, context: &C) -> Response {
                let G = or_respond!(G::guard(parts, context));
                $( let $head = or_respond!($head::from_request_parts(parts, context)); )*
                let $last = or_respond!($last::from_request(parts, body, context));
                self(G, $($head,)* $last).await.into_response()
            }

            fn describe(operation: &mut OperationCx<'_>) {
                <G as Describe>::describe(operation);
                $( describe_argument!($head as FromRequestParts, operation); )*
                describe_argument!($last as FromRequest, operation);
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
            #[allow(non_snake_case, unused_variables)]
            async fn call(self, parts: &mut Parts, _body: Body, context: &C) -> Response {
                let G = or_respond!(G::guard(parts, context));
                $( let $head = or_respond!($head::from_request_parts(parts, context)); )*
                let $last = or_respond!($last::from_request_parts(parts, context));
                self(G, $($head,)* $last).await.into_response()
            }

            fn describe(operation: &mut OperationCx<'_>) {
                <G as Describe>::describe(operation);
                $( describe_argument!($head as FromRequestParts, operation); )*
                describe_argument!($last as FromRequestParts, operation);
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
    async fn call(self, parts: &mut Parts, body: Body, context: &C) -> Response {
        let _ = (parts, body, context);
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
    async fn call(self, parts: &mut Parts, _body: Body, context: &C) -> Response {
        let G = or_respond!(G::guard(parts, context));
        self(G).await.into_response()
    }

    /// The guard declares its refusals itself, as in `impl_handler!`.
    fn describe(operation: &mut OperationCx<'_>) {
        <G as Describe>::describe(operation);
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
