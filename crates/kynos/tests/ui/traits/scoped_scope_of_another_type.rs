//! A scope set whose scope type is not the scheme's.
use kynos::security::{
    Scope,
    auth::{Scoped, Scopes},
    schemes::Bearer,
};

#[derive(Clone, Copy)]
enum Grant {
    ReadReports,
}

impl Scope for Grant {
    fn name(&self) -> &'static str {
        match self {
            Self::ReadReports => "reports:read",
        }
    }
}

struct ReadReports;

impl Scopes for ReadReports {
    type Scope = Grant;
    const REQUIRED: &'static [Grant] = &[Grant::ReadReports];
}

fn main() {
    let _ = std::marker::PhantomData::<Scoped<Bearer, ReadReports>>;
}
