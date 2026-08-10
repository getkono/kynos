use crate::di::{Provides, inject::Inject};

#[derive(Clone, Debug, PartialEq)]
struct Pool(u32);

#[derive(Clone, Debug, PartialEq)]
struct Cache(&'static str);

/// What `#[derive(Provider)]` emits: one implementation per field.
struct App {
    pool: Pool,
    cache: Cache,
}

impl Provides<Pool> for App {
    fn provide(&self) -> Pool {
        self.pool.clone()
    }
}

impl Provides<Cache> for App {
    fn provide(&self) -> Cache {
        self.cache.clone()
    }
}

fn app() -> App {
    App {
        pool: Pool(7),
        cache: Cache("local"),
    }
}

#[test]
fn a_context_supplies_each_of_its_dependencies() {
    let app = app();
    assert_eq!(Provides::<Pool>::provide(&app), Pool(7));
    assert_eq!(Provides::<Cache>::provide(&app), Cache("local"));
}

/// An application with one dependency needs no wrapper struct: the dependency
/// is its own context.
#[test]
fn every_context_provides_itself() {
    let pool = Pool(7);
    assert_eq!(Provides::<Pool>::provide(&pool), Pool(7));
}

/// The reflexive implementation must not collide with a derived one. It would
/// only unify if the context type and the field type were the same type, which
/// no `#[derive(Provider)]` output can produce.
#[test]
fn the_reflexive_implementation_leaves_room_for_derived_ones() {
    fn resolves<C: Provides<Pool> + Provides<Cache>>(context: &C) -> (Pool, Cache) {
        (context.provide(), context.provide())
    }

    assert_eq!(resolves(&app()), (Pool(7), Cache("local")));
}

#[test]
fn injecting_unwraps_to_the_value() {
    assert_eq!(Inject(Pool(7)).into_inner(), Pool(7));
}
