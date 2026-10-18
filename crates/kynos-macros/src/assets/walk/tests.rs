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
    let folded = fold_encodings(&paths.iter().copied().map(found).collect::<Vec<_>>());

    let mut reversed: Vec<&str> = paths.to_vec();
    reversed.reverse();
    let other = fold_encodings(&reversed.iter().copied().map(found).collect::<Vec<_>>());

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

/// What one fold must produce: each resource in order, with the file each of
/// its codings came from, in order.
type Shape = &'static [(&'static str, &'static [(&'static str, &'static str)])];

/// The fold, pinned case by case: which file each input became, and in which
/// order, compared whole rather than by membership.
///
/// Each row's input is out of order on purpose, so a row passes only where the
/// fold sorted it.
#[test]
fn each_fold_produces_exactly_its_shape() {
    let rows: &[(&str, &[&str], Shape)] = &[
        (
            "resources come out in path order, nested paths among them",
            &["nested/b.css", "c.map", "a.txt"],
            &[("a.txt", &[]), ("c.map", &[]), ("nested/b.css", &[])],
        ),
        (
            "byte order, so an upper-case name sorts first",
            &["b.txt", "B.txt", "a/z.txt", "a.txt"],
            &[
                ("B.txt", &[]),
                ("a.txt", &[]),
                ("a/z.txt", &[]),
                ("b.txt", &[]),
            ],
        ),
        (
            "each coding attaches to its own base and to no other",
            &["b.js.gz", "a.js.br", "b.js", "a.js"],
            &[
                ("a.js", &[("br", "a.js.br")]),
                ("b.js", &[("gzip", "b.js.gz")]),
            ],
        ),
        (
            "a coding in a directory attaches to the base beside it, not to a \
             same-named file elsewhere",
            &["app.js", "nested/app.js.gz", "nested/app.js"],
            &[
                ("app.js", &[]),
                ("nested/app.js", &[("gzip", "nested/app.js.gz")]),
            ],
        ),
        (
            "a base with no extension takes its codings too",
            &["LICENSE.zst", "LICENSE"],
            &[("LICENSE", &[("zstd", "LICENSE.zst")])],
        ),
        (
            "codings arrive br, gzip, zstd whatever order the walk found them in",
            &["app.js.zst", "app.js.br", "app.js", "app.js.gz"],
            &[(
                "app.js",
                &[
                    ("br", "app.js.br"),
                    ("gzip", "app.js.gz"),
                    ("zstd", "app.js.zst"),
                ],
            )],
        ),
        (
            "a coding of a folded coding stays a resource, in its own place",
            &["app.js.br.gz", "app.js.br", "app.js", "app.jsx"],
            &[
                ("app.js", &[("br", "app.js.br")]),
                ("app.js.br.gz", &[]),
                ("app.jsx", &[]),
            ],
        ),
        (
            "a coding whose base is itself a coding of nothing folds into it",
            &["archive.tar.gz.br", "archive.tar.gz"],
            &[("archive.tar.gz", &[("br", "archive.tar.gz.br")])],
        ),
        (
            "a suffix that is not a stored coding is part of the name",
            &["app.js.bz2", "app.js"],
            &[("app.js", &[]), ("app.js.bz2", &[])],
        ),
    ];

    for (description, paths, expected) in rows {
        let folded = fold(paths);
        let shape: Vec<(&str, Vec<(&str, &str)>)> = folded
            .iter()
            .map(|file| {
                (
                    file.path.as_str(),
                    file.encodings
                        .iter()
                        .map(|encoded| {
                            let from = encoded
                                .absolute
                                .strip_prefix("/root/")
                                .expect("`found` roots every file");
                            (encoded.coding, from)
                        })
                        .collect(),
                )
            })
            .collect();
        let expected: Vec<(&str, Vec<(&str, &str)>)> = expected
            .iter()
            .map(|(path, codings)| (*path, codings.to_vec()))
            .collect();
        assert_eq!(shape, expected, "{description}");

        // Each file keeps its own validator, whichever slot it landed in.
        for file in &folded {
            assert_eq!(file.etag, format!("\"{}\"", file.path), "{description}");
            for encoded in &file.encodings {
                assert_eq!(
                    encoded.etag,
                    format!("\"{}\"", &encoded.absolute["/root/".len()..]),
                    "{description}"
                );
            }
        }
    }
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
