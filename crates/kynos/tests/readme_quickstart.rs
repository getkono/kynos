//! The README's Quickstart compiles against exactly what its Install step adds.
//!
//! One reason: both sections address one reader, who runs the `cargo add`
//! lines and pastes the program, and nothing else builds that program under
//! those dependencies alone. A doctest or an example sees every
//! dev-dependency, so it compiles a Quickstart that names a crate Install
//! never adds. This generates a package outside the workspace whose manifest
//! holds what the Install block adds and nothing more, and checks it offline
//! against the workspace's lockfile, with `kynos` taken from this checkout.

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const README: &str = include_str!("../../../README.md");
const CRATE_README: &str = include_str!("../README.md");

/// The body of the first fenced block tagged `language` under `heading`.
fn fenced_block<'a>(markdown: &'a str, heading: &str, language: &str) -> &'a str {
    let section = markdown
        .split_once(&format!("\n{heading}\n"))
        .unwrap_or_else(|| panic!("no `{heading}` heading"))
        .1;
    let section = section.split("\n## ").next().unwrap_or(section);
    let opening = format!("```{language}\n");
    let body = section
        .split_once(&opening)
        .unwrap_or_else(|| panic!("no ```{language} block under `{heading}`"))
        .1;
    body.split_once("```")
        .unwrap_or_else(|| panic!("the ```{language} block under `{heading}` is not closed"))
        .0
}

/// The dependencies an Install block adds: each crate and the features it
/// enables.
///
/// The grammar is the one line shape a reader copies, `cargo add CRATE` with an
/// optional `--features A,B`, and anything else panics: a line this cannot
/// read is a line this test would otherwise silently fail to install.
fn installed(block: &str) -> BTreeMap<String, Vec<String>> {
    let mut dependencies = BTreeMap::new();
    for line in block.lines().filter(|line| !line.trim().is_empty()) {
        let words: Vec<&str> = line.split_whitespace().collect();
        let (name, features) = match words.as_slice() {
            ["cargo", "add", name] => (*name, Vec::new()),
            ["cargo", "add", name, "--features", features] => (
                *name,
                features.split(',').map(str::to_owned).collect::<Vec<_>>(),
            ),
            _ => panic!("an Install line this test cannot read: `{line}`"),
        };
        assert!(
            dependencies.insert(name.to_owned(), features).is_none(),
            "Install adds `{name}` twice",
        );
    }
    dependencies
}

/// The manifest `cargo add` would leave behind, with `kynos` from this
/// checkout and every other crate at whatever version the lockfile holds.
fn manifest(dependencies: &BTreeMap<String, Vec<String>>) -> String {
    let kynos = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut manifest = String::from(
        "[package]\nname = \"quickstart\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
         publish = false\n\n# Outside the Kynos workspace, as a reader's package is.\n\
         [workspace]\n\n[dependencies]\n",
    );
    for (name, features) in dependencies {
        let source = if name == "kynos" {
            format!("path = '{}'", kynos.display())
        } else {
            String::from("version = \"*\"")
        };
        let features = features
            .iter()
            .map(|feature| format!("\"{feature}\""))
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(manifest, "{name} = {{ {source}, features = [{features}] }}")
            .expect("a String accepts every write");
    }
    manifest
}

#[test]
fn the_quickstart_compiles_against_what_install_adds() {
    let dependencies = installed(fenced_block(README, "## Install", "bash"));
    assert!(
        dependencies.contains_key("kynos"),
        "Install does not add kynos: {dependencies:?}",
    );

    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("readme_quickstart");
    let package = root.join("package");
    fs::create_dir_all(package.join("src")).expect("the package directory");
    fs::write(package.join("Cargo.toml"), manifest(&dependencies)).expect("the manifest");
    fs::write(
        package.join("src/main.rs"),
        fenced_block(README, "## Quickstart", "rust"),
    )
    .expect("the Quickstart");
    // The workspace's lockfile, so the check resolves offline to versions the
    // workspace already builds; Cargo prunes what this package does not use.
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock"),
        package.join("Cargo.lock"),
    )
    .expect("the lockfile");

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| env!("CARGO").into());
    let output = Command::new(cargo)
        .args(["check", "--offline", "--quiet", "--manifest-path"])
        .arg(package.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", root.join("target"))
        // A coverage or lint run's flags would reach this build too; the
        // reader's build has none.
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("CARGO_BUILD_RUSTFLAGS")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("LLVM_PROFILE_FILE")
        .output()
        .expect("cargo runs");
    assert!(
        output.status.success(),
        "the Quickstart does not compile against what Install adds:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// The crate's own README, which is what crates.io shows, installs the same
/// dependencies the repository README's Quickstart was checked against.
#[test]
fn the_crate_readme_installs_what_the_repository_readme_does() {
    assert_eq!(
        installed(fenced_block(CRATE_README, "## Install", "bash")),
        installed(fenced_block(README, "## Install", "bash")),
    );
}
