//! Reading a directory at expansion time.

use std::path::{Path, PathBuf};

use kynos_openapi::PathTemplate;

use crate::assets::args::AssetArgs;

/// How deep a set may nest.
///
/// A bound rather than a limit anyone will meet: a build output nests three or
/// four levels, and a symlink loop the walker followed would otherwise not
/// terminate.
const MAX_DEPTH: usize = 32;

/// How many files a set may hold.
const MAX_FILES: usize = 100_000;

/// One embedded file.
pub(super) struct Embedded {
    /// The relative, `/`-separated path it serves at.
    pub(super) path: String,
    /// The absolute path `include_bytes!` is given.
    pub(super) absolute: String,
    /// The quoted entity tag.
    pub(super) etag: String,
}

/// What the walk found.
pub(super) struct Walked {
    pub(super) files: Vec<Embedded>,
    pub(super) total_bytes: usize,
}

/// Walks `args.dir`, relative to the crate being compiled.
pub(super) fn walk(args: &AssetArgs) -> syn::Result<Walked> {
    let root = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR")
            .map_err(|_| syn::Error::new(args.dir.span(), "`CARGO_MANIFEST_DIR` is not set"))?,
    )
    .join(args.dir.value());

    let root = root.canonicalize().map_err(|error| {
        syn::Error::new(
            args.dir.span(),
            format!("`{}` could not be read: {error}", args.dir.value()),
        )
    })?;

    let mut files = Vec::new();
    collect(args, &root, &root, 0, &mut files)?;

    if files.is_empty() {
        return Err(syn::Error::new(
            args.dir.span(),
            format!(
                "`{}` holds no servable file; an asset set that serves nothing is one no route \
                 needs",
                args.dir.value()
            ),
        ));
    }

    // Sorted, so the emitted set is byte-identical across machines. A document
    // that differs by directory-read order is one `--check` mode cannot use.
    files.sort_by(|left, right| left.path.cmp(&right.path));

    let total_bytes = files.iter().map(|file| file.byte_count).sum();

    Ok(Walked {
        files: files
            .into_iter()
            .map(|file| Embedded {
                path: file.path,
                absolute: file.absolute,
                etag: file.etag,
            })
            .collect(),
        total_bytes,
    })
}

/// One file, before the count is folded away.
struct Found {
    path: String,
    absolute: String,
    etag: String,
    byte_count: usize,
}

fn collect(
    args: &AssetArgs,
    root: &Path,
    directory: &Path,
    depth: usize,
    files: &mut Vec<Found>,
) -> syn::Result<()> {
    if depth > MAX_DEPTH {
        return Err(syn::Error::new(
            args.dir.span(),
            format!(
                "`{}` nests deeper than {MAX_DEPTH} levels",
                args.dir.value()
            ),
        ));
    }

    let entries = std::fs::read_dir(directory).map_err(|error| {
        syn::Error::new(
            args.dir.span(),
            format!("`{}` could not be read: {error}", directory.display()),
        )
    })?;

    for entry in entries {
        let entry = entry.map_err(|error| {
            syn::Error::new(args.dir.span(), format!("a directory entry: {error}"))
        })?;
        let path = entry.path();

        let name = entry.file_name().to_string_lossy().into_owned();
        // A dotfile is not part of a build output, and `.git` is the case that
        // matters: embedding it would put a repository in a binary.
        if name.starts_with('.') {
            continue;
        }

        // `metadata` follows symlinks; `symlink_metadata` does not. Following
        // one is how a set escapes its own directory, so links are skipped.
        let kind = entry.file_type().map_err(|error| {
            syn::Error::new(args.dir.span(), format!("a directory entry: {error}"))
        })?;
        if kind.is_symlink() {
            continue;
        }

        if kind.is_dir() {
            collect(args, root, &path, depth + 1, files)?;
            continue;
        }

        let relative = path
            .strip_prefix(root)
            .map_err(|_| syn::Error::new(args.dir.span(), "an entry outside the asset directory"))?
            .to_string_lossy()
            .replace('\\', "/");

        if excluded(args, &relative, &name) {
            continue;
        }

        // Refused at compile time rather than waived. A static asset Kynos
        // cannot describe is one it will not serve, and the alternative is a
        // document that quietly omits a file the service answers for.
        if PathTemplate::parse(&format!("/{relative}")).is_err() {
            return Err(syn::Error::new(
                args.dir.span(),
                format!(
                    "`{relative}` has a name no path template can express, so it cannot be \
                     described; rename it, or add it to `exclude`"
                ),
            ));
        }

        let bytes = std::fs::read(&path).map_err(|error| {
            syn::Error::new(
                args.dir.span(),
                format!("`{}` could not be read: {error}", path.display()),
            )
        })?;

        files.push(Found {
            etag: format!("\"{:016x}\"", fnv1a(&bytes)),
            byte_count: bytes.len(),
            path: relative,
            absolute: path.to_string_lossy().into_owned(),
        });

        if files.len() > MAX_FILES {
            return Err(syn::Error::new(
                args.dir.span(),
                format!("`{}` holds more than {MAX_FILES} files", args.dir.value()),
            ));
        }
    }

    Ok(())
}

/// Whether `exclude` names this file.
///
/// An entry beginning with a dot is an extension; anything else is a relative
/// path or a bare file name.
fn excluded(args: &AssetArgs, relative: &str, name: &str) -> bool {
    args.exclude.iter().any(|entry| {
        let entry = entry.value();
        if entry.starts_with('.') {
            relative.ends_with(&entry)
        } else {
            relative == entry || name == entry
        }
    })
}

/// FNV-1a over the contents, with the length folded in.
///
/// An entity tag is a cache validator rather than a security primitive: RFC 9110
/// section 8.8.3 asks only that it change when the representation does, and
/// nothing verifies it. A cryptographic hash would mean `sha2` or `blake3`,
/// both of which arrive with the `unsafe` this workspace forbids, for a
/// 64-bit token.
///
/// The collision bound that matters is over one file's *history* rather than
/// over the set: a client compares a tag against the same URL it got it from.
fn fnv1a(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let hashed = bytes.iter().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(PRIME)
    });

    // Two files differing only in trailing zero bytes hash alike under FNV-1a
    // alone, which a build output can produce.
    (hashed ^ (bytes.len() as u64)).wrapping_mul(PRIME)
}
