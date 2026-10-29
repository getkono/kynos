use quote::quote;
use syn::DeriveInput;

use super::doc_string;

fn item(declaration: proc_macro2::TokenStream) -> DeriveInput {
    syn::parse2(declaration).expect("an item")
}

/// A description is CommonMark, so the indentation a doc comment's Markdown
/// depends on reaches it: an indented code block, a nested list item and a
/// two-space hard break each survive, and only the indentation every line
/// shares - rustdoc's one space after `///` - is removed.
#[test]
fn a_doc_comment_keeps_its_markdown_indentation() {
    let input = item(quote! {
        /// A user.
        ///
        /// Created with:
        ///
        ///     let user = User::new();
        ///
        /// - roles
        ///   - admin
        ///
        #[doc = " ends here  "]
        /// and resumes here.
        ///
        struct User;
    });

    assert_eq!(
        doc_string(&input.attrs).as_deref(),
        Some(
            "A user.\n\nCreated with:\n\n    let user = User::new();\n\n- roles\n  - admin\n\n\
             ends here  \nand resumes here."
        )
    );
}

/// Indentation is removed only where every line shares it: a doc attribute
/// written without rustdoc's leading space keeps the indentation of the lines
/// beside it, and the lines one attribute's text spans are lines of their own.
#[test]
fn only_the_indentation_every_line_shares_is_removed() {
    let input = item(quote! {
        #[doc = "Run it:"]
        #[doc = ""]
        #[doc = "    cargo run\n    cargo test"]
        struct Command;
    });

    assert_eq!(
        doc_string(&input.attrs).as_deref(),
        Some("Run it:\n\n    cargo run\n    cargo test")
    );
}
