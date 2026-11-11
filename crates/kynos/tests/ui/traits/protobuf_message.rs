//! A `Protobuf` over a message that derives only `prost::Message` does not
//! compile, and the diagnostic names the derive that lets Kynos carry it.
//!
//! The readable mistake is the one every message written before Kynos owned
//! the contract makes: `prost`'s derive alone.

use kynos::{
    Schema,
    extract::{FromRequest, body::protobuf::Protobuf},
};

#[derive(Clone, PartialEq, prost::Message, Schema)]
struct Reading {
    #[prost(uint64, tag = "1")]
    value: u64,
}

fn from_request<C, T: FromRequest<C>>() {}

fn main() {
    from_request::<(), Protobuf<Reading>>();
}
