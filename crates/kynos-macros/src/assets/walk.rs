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

/// Filename suffixes that mark a stored content coding, and the coding token
/// each one names.
///
/// A *stored* coding, not one Kynos produces: a build pipeline writes
/// `app.js.br` beside `app.js`, and the set serves whichever the client
/// accepts. That is why there is no compressor here and no dependency on one --
/// and why each stored form can carry its own strong validator, which is the
/// whole point. RFC 9110 section 8.8.1 forbids one strong entity tag from
/// naming two representations, so a coding without its own tag is a coding that
/// cannot be ranged over.
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

    // Sorted, so the emitted set is byte-identical across machines. A document
    // that differs by directory-read order is one `--check` mode cannot use.
    files.sort_by(|left, right| left.path.cmp(&right.path));

    let total_bytes = files.iter().map(|file| file.byte_count).sum();

    Ok(Walked {
        files: fold_encodings(files),
        total_bytes,
    })
}

/// Attaches each stored coding to the file it is a coding *of*.
///
/// `app.js.br` beside `app.js` is not a second resource; it is the same
/// representation in another content coding, and serving it at its own path
/// would describe an operation no client asks for while leaving the one it does
/// ask for unable to answer `Accept-Encoding`.
///
/// A coding whose base file is absent stays a file of its own. Somebody shipping
/// `archive.tar.gz` for download means the path, not the coding, and a set that
/// swallowed it would serve nothing at the URL the page links to.
///
/// "Absent" means absent from the *resources*, not merely from the directory: a
/// coding folds into its base only when that base is itself served. So `app.js`,
/// `app.js.br` and `app.js.br.gz` become `app.js` carrying `br`, plus
/// `app.js.br.gz` at its own path -- the `br` form it names is no longer a
/// resource to hang a coding on, and the alternative is dropping a file the
/// build wrote with no diagnostic anywhere to say so. Every file ends up either
/// a resource or one coding of one; never neither, and never both.
///
/// That is why classification and folding are the same pass. Deciding what is a
/// coding against the raw path set, and then attaching against the resources, is
/// the same question asked of two different sets -- and the file a coding of a
/// coding falls between is the answer they disagree about.
///
/// Shortest name first, because stripping a coding suffix leaves a strictly
/// shorter name: a base is always classified before anything encoding it.
/// `files` may arrive in any order, and the result is put back in path order
/// regardless, so the emitted set is byte-identical across machines.
fn fold_encodings(files: Vec<Found>) -> Vec<Embedded> {
    let mut order: Vec<&Found> = files.iter().collect();
    // Length ties are broken by the path, so the order -- and with it which of
    // two same-length names is decided first -- does not depend on the walk.
    order.sort_by(|left, right| {
        left.path
            .len()
            .cmp(&right.path.len())
            .then_with(|| left.path.cmp(&right.path))
    });

    let mut embedded: Vec<Embedded> = Vec::new();
    let mut index_of: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();

    for file in order {
        // A coding of something already known to be a resource folds in.
        // Anything else -- a base outside the set, or one that folded away
        // itself -- is a resource, since its path is all that is left of it.
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

    // Deterministic, and it decides which coding a tie goes to: the order here
    // is the order `preferred` is offered, so `br` before `gzip` before `zstd`
    // would be the wrong default -- brotli is the smallest and the one a client
    // naming all three at equal weight should get.
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

#[cfg(test)]
mod tests {
    use super::{Embedded, Found, fold_encodings};

    /// One walked file, with a tag that names it so a mix-up is readable.
    fn found(path: &str) -> Found {
        Found {
            path: path.to_owned(),
            absolute: format!("/root/{path}"),
            etag: format!("\"{path}\""),
            byte_count: 0,
        }
    }

    /// Every input file is either a resource or a coding of one, never neither.
    ///
    /// The invariant the folding exists to preserve: a set that swallowed a
    /// file would serve nothing at a URL the build wrote, with no diagnostic
    /// anywhere to say so.
    fn assert_nothing_lost(paths: &[&str], embedded: &[Embedded]) {
        let codings: usize = embedded.iter().map(|file| file.encodings.len()).sum();
        assert_eq!(
            paths.len(),
            embedded.len() + codings,
            "{paths:?} folded to {:?} plus {codings} codings",
            embedded.iter().map(|file| &file.path).collect::<Vec<_>>()
        );
    }

    /// Folds `paths`, and asserts the result does not depend on their order.
    fn fold(paths: &[&str]) -> Vec<Embedded> {
        let folded = fold_encodings(paths.iter().copied().map(found).collect());

        let mut reversed: Vec<&str> = paths.to_vec();
        reversed.reverse();
        let other = fold_encodings(reversed.iter().copied().map(found).collect());

        let shape = |files: &[Embedded]| {
            files
                .iter()
                .map(|file| {
                    (
                        file.path.clone(),
                        file.encodings
                            .iter()
                            .map(|encoded| encoded.coding)
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            shape(&folded),
            shape(&other),
            "the fold depended on the order `files` arrived in"
        );

        assert_nothing_lost(paths, &folded);
        folded
    }

    /// A stored coding beside its base is the same resource in another coding.
    #[test]
    fn a_coding_folds_into_the_file_it_encodes() {
        let paths = ["app.js", "app.js.br"];
        let folded = fold(&paths);

        assert_eq!(folded.len(), 1);
        assert_eq!(folded[0].path, "app.js");
        assert_eq!(folded[0].absolute, "/root/app.js");
        assert_eq!(
            folded[0]
                .encodings
                .iter()
                .map(|encoded| (encoded.coding, encoded.absolute.as_str()))
                .collect::<Vec<_>>(),
            vec![("br", "/root/app.js.br")]
        );
    }

    /// A coding of a coding is a resource, because the form it encodes is not.
    ///
    /// `app.js.br` folds away, so there is nothing for `app.js.br.gz` to attach
    /// to -- and a file with nowhere to attach is served at its own path rather
    /// than dropped.
    #[test]
    fn a_coding_whose_base_is_not_a_resource_stays_a_resource() {
        let paths = ["app.js", "app.js.br", "app.js.br.gz"];
        let folded = fold(&paths);

        assert_eq!(
            folded.iter().map(|file| &file.path).collect::<Vec<_>>(),
            vec!["app.js", "app.js.br.gz"]
        );

        let base = &folded[0];
        assert_eq!(
            base.encodings
                .iter()
                .map(|encoded| encoded.coding)
                .collect::<Vec<_>>(),
            vec!["br"]
        );
        assert!(
            folded[1].encodings.is_empty(),
            "nothing encodes `app.js.br.gz`"
        );
    }

    /// A coding whose base is absent from the directory means the path.
    #[test]
    fn a_coding_without_a_base_is_a_file_of_its_own() {
        let paths = ["archive.tar.gz"];
        let folded = fold(&paths);

        assert_eq!(folded.len(), 1);
        assert_eq!(folded[0].path, "archive.tar.gz");
        assert!(folded[0].encodings.is_empty());
    }

    /// Several codings of one file arrive in the order `preferred` offers them.
    #[test]
    fn the_codings_of_one_file_are_ordered_smallest_first() {
        let paths = ["app.js", "app.js.gz", "app.js.zst", "app.js.br"];
        let folded = fold(&paths);

        assert_eq!(folded.len(), 1);
        assert_eq!(
            folded[0]
                .encodings
                .iter()
                .map(|encoded| encoded.coding)
                .collect::<Vec<_>>(),
            vec!["br", "gzip", "zstd"]
        );
    }
}
