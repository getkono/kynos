//! A variant whose own name an earlier variant's `alias` claims: serde reads
//! `"Stop"` as `Signal::Start`, so it writes `Signal::Stop` under a name that
//! reads back as another variant, and no `oneOf` is true of both. serde accepts
//! this declaration, so the refusal is the derive's own.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
enum Signal {
    #[serde(alias = "Stop")]
    Start,
    Stop,
}

fn main() {}
