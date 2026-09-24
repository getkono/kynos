//! A transparent tuple struct is its one described member. The `PhantomData`
//! beside it is neither written by serde nor described, so it demands no
//! `Schema` of its own, which nothing could satisfy.

use std::marker::PhantomData;

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct Id<T>(u64, PhantomData<T>);

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<Id<u8>>();
}
