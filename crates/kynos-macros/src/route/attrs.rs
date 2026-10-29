//! Reading a handler's doc comment as an operation's summary and description.

use crate::doc::doc_markdown;

/// Splits a doc comment into its summary and description.
///
/// The first paragraph becomes the operation's `summary` and the remainder its
/// `description`, matching how the specification distinguishes the two. The
/// summary is plain text, so its lines join into one; the description is
/// CommonMark, so it keeps the indentation its Markdown depends on, less what
/// every line of the comment shares (see [`doc_markdown`]).
pub(crate) fn split_doc(texts: &[String]) -> (Option<String>, Option<String>) {
    let lines = doc_markdown(texts);
    let first_blank = lines.iter().position(|line| line.is_empty());
    let (summary, rest) = match first_blank {
        Some(index) => (&lines[..index], &lines[index + 1..]),
        None => (&lines[..], &[][..]),
    };

    let summary = summary
        .iter()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join(" ");
    let rest = rest.join("\n");
    let rest = rest.trim_start_matches('\n').trim_end();
    (
        (!summary.is_empty()).then_some(summary),
        (!rest.is_empty()).then(|| rest.to_owned()),
    )
}
