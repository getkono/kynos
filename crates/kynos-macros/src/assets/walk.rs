//! Reading a directory at expansion time.

use std::path::{Path, PathBuf};

use kynos_openapi::PathTemplate;

use crate::assets::args::AssetArgs;

/// How deep a set may nest; a backstop that guarantees the walk terminates.
const MAX_DEPTH: usize = 32;

/// How many files a set may hold.
const MAX_FILES: usize = 100_000;

/// Filename suffixes that mark a stored content coding, and the coding token
/// each one names.
///
/// Codings are written by the build, not produced here; each carries its own
/// strong validator, since RFC 9110 section 8.8.1 forbids one strong entity tag
/// naming two representations.
const STORED_CODINGS: &[(&str, &str)] = &[(".br", "br"), (".gz", "gzip"), (".zst", "zstd")];

/// The coding `name` is a stored form of, and the name it encodes.
fn stored_coding(name: &str) -> Option<(&'static str, &str)> {
    STORED_CODINGS
        .iter()
        .find_map(|(suffix, coding)| name.strip_suffix(suffix).map(|base| (*coding, base)))
}

/// One stored content coding of a file.
pub(super) struct Encoded {
    /// The coding token, as `Content-Encoding` spells it.
    pub(super) coding: &'static str,
    /// The absolute path `include_bytes!` is given.
    pub(super) absolute: String,
    /// The quoted entity tag, minted from *these* octets.
    pub(super) etag: String,
}

/// One embedded file.
pub(super) struct Embedded {
    /// The relative, `/`-separated path it serves at.
    pub(super) path: String,
    /// The absolute path `include_bytes!` is given.
    pub(super) absolute: String,
    /// The quoted entity tag.
    pub(super) etag: String,
    /// Stored codings of the same representation, each with its own validator.
    pub(super) encodings: Vec<Encoded>,
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

    // Sorted, so the emitted set is byte-identical across machines.
    files.sort_by(|left, right| left.path.cmp(&right.path));

    let total_bytes = files.iter().map(|file| file.byte_count).sum();

    Ok(Walked {
        files: fold_encodings(&files),
        total_bytes,
    })
}

/// Attaches each stored coding to the file it is a coding *of*.
///
/// A coding folds in only when its base is itself a served resource; otherwise
/// it stays a file at its own path (`archive.tar.gz`, or `app.js.br.gz` once
/// `app.js.br` folded away). Every file ends up a resource or one coding of one,
/// never neither or both, which is why classifying and folding are one pass.
///
/// Shortest name first, so a base is classified before anything encoding it;
/// the result is returned in path order.
fn fold_encodings(files: &[Found]) -> Vec<Embedded> {
    let mut order: Vec<&Found> = files.iter().collect();
    // Length ties broken by path, so the order does not depend on the walk.
    order.sort_by(|left, right| {
        left.path
            .len()
            .cmp(&right.path.len())
            .then_with(|| left.path.cmp(&right.path))
    });

    let mut embedded: Vec<Embedded> = Vec::new();
    let mut index_of: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();

    for file in order {
        // A coding of a known resource folds in; anything else is a resource.
        if let Some((coding, index)) = stored_coding(&file.path)
            .and_then(|(coding, base)| Some((coding, *index_of.get(base)?)))
        {
            embedded[index].encodings.push(Encoded {
                coding,
                absolute: file.absolute.clone(),
                etag: file.etag.clone(),
            });
            continue;
        }

        index_of.insert(&file.path, embedded.len());
        embedded.push(Embedded {
            path: file.path.clone(),
            absolute: file.absolute.clone(),
            etag: file.etag.clone(),
            encodings: Vec::new(),
        });
    }

    embedded.sort_by(|left, right| left.path.cmp(&right.path));

    // `STORED_CODINGS` order, which is the order `preferred` breaks a tie in.
    for file in &mut embedded {
        file.encodings.sort_by_key(|encoded| {
            STORED_CODINGS
                .iter()
                .position(|(_, coding)| *coding == encoded.coding)
                .unwrap_or(usize::MAX)
        });
    }

    embedded
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
        // Dotfiles (notably `.git`) are never embedded.
        if name.starts_with('.') {
            continue;
        }

        // Symlinks are skipped so a set cannot escape its directory.
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

        // Refused: a file the document cannot describe is not served.
        if PathTemplate::parse(format!("/{relative}")).is_err() {
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
/// Not cryptographic: RFC 9110 section 8.8.3 asks only that an entity tag
/// change when the representation does.
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

#[cfg(test)]
mod tests;
