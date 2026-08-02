//! An idiomatic, performance-focused framework for building REST APIs with
//! first-class OpenAPI 3.1 support.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_metadata_matches_workspace() {
        assert_eq!(env!("CARGO_PKG_NAME"), "kynos");
        assert_eq!(env!("CARGO_PKG_VERSION"), "0.1.0");
    }
}
