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
