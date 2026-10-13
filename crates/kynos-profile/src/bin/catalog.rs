//! Prints the catalog `scripts/profile_report.py` reads its bases from: one
//! line per scenario, `name`, `base` and `driver`, tab-separated, with an
//! empty `base` where a scenario names none.
//!
//! A binary rather than a parse of `lib.rs`, so the report reads the catalog
//! the benchmark was built from rather than a second spelling of it.

use std::io::{self, Write};

fn main() -> io::Result<()> {
    let mut out = io::stdout().lock();
    for scenario in kynos_profile::catalog() {
        writeln!(
            out,
            "{}\t{}\t{:?}",
            scenario.name,
            scenario.base.unwrap_or_default(),
            scenario.driver
        )?;
    }
    Ok(())
}
