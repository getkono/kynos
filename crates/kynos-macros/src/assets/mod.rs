//! `assets!`: a directory, compiled into the binary as a described set.
//!
//! ```text
//! assets! {
//!     /// The built single-page app.
//!     pub struct Site;
//!     dir = "dist",
//!     exclude = [".map", "robots.txt"],
//!     warn_over = "4MiB",
//! }
//! ```
//!
//! # Why the contents are `include_bytes!` rather than byte literals
//!
//! A proc macro reading a file leaves no trace the build system can see:
//! `proc_macro::tracked_path` is nightly, so an expansion that embedded the
//! bytes directly would serve whatever it read the first time it ran, forever.
//! `include_bytes!` registers a compiler file dependency, so a *changed* asset
//! rebuilds.
//!
//! Adding or removing a file still does not, because membership is not a file's
//! contents. `examples/assets.rs` shows the `cargo::rerun-if-changed` line that
//! closes it, which is a `build.rs` away rather than something the macro can do.

mod args;
mod walk;

use proc_macro::TokenStream;
use quote::quote;
use syn::parse_macro_input;

use crate::assets::{args::AssetArgs, walk::Walked};

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(item as AssetArgs);
    match expand_inner(&args) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

pub(crate) fn expand_inner(args: &AssetArgs) -> syn::Result<proc_macro2::TokenStream> {
    let walked = walk::walk(args)?;

    let name = &args.name;
    let visibility = &args.visibility;
    let docs = &args.docs;

    let entries = walked.files.iter().map(|file| {
        let path = &file.path;
        let absolute = &file.absolute;
        let etag = &file.etag;
        quote! {
            ::kynos::router::assets::Asset::embedded(
                #path,
                ::core::include_bytes!(#absolute),
                #etag,
            )
        }
    });

    let total = walked.total_bytes;
    let count = walked.files.len();
    let guard = size_guard(args, &walked);

    Ok(quote! {
        #(#docs)*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        #visibility struct #name;

        impl #name {
            /// Every embedded file, in sorted relative-path order.
            pub const ASSETS: &'static [::kynos::router::assets::Asset] = &[#(#entries),*];

            /// The total embedded byte count.
            pub const TOTAL_BYTES: usize = #total;

            /// How many files were embedded.
            pub const COUNT: usize = #count;

            /// This set, ready to mount.
            #[must_use]
            #visibility fn assets() -> ::kynos::router::assets::AssetSet {
                ::kynos::router::assets::AssetSet::embedded(Self::ASSETS)
            }
        }

        #guard
    })
}

/// The compiler warning an oversized set emits, spanned at the `dir` literal.
///
/// `proc_macro::Diagnostic` is nightly, so the warning is produced by *using* an
/// item this expansion marked `#[deprecated]` — which is warn-by-default, fires
/// at the use rather than the definition, and carries whatever span the use has.
/// That is the one way to say something non-fatal from a stable proc macro.
fn size_guard(args: &AssetArgs, walked: &Walked) -> Option<proc_macro2::TokenStream> {
    let threshold = args.warn_over?;
    if walked.total_bytes <= threshold {
        return None;
    }

    let total = walked.total_bytes;
    let directory = args.dir.value();
    let suggestion = suggest(walked.total_bytes);
    let note = format!(
        "`assets!` embedded {total} bytes from `{directory}` into this binary, past the \
         {threshold}-byte threshold. Binaries this size are slow to link and costly to ship; \
         serve them from a CDN, or from disk with `assets-fs`. Raise the threshold with \
         `warn_over = \"{suggestion}\"`, or turn the check off with `warn_over = \"none\"`."
    );

    // Spanned at the `dir` literal, so the warning points at the directory that
    // caused it rather than at the macro call as a whole.
    let span = args.dir.span();
    let marker = syn::Ident::new("this_embedded_asset_set_is_large", span);

    Some(quote::quote_spanned! { span =>
        const _: () = {
            #[deprecated(note = #note)]
            #[allow(non_upper_case_globals)]
            const #marker: () = ();

            // Deliberately *not* `#[allow(deprecated)]`: using the item is the
            // whole mechanism, and allowing the lint here would silence the
            // warning this exists to produce.
            let _ = #marker;
        };
    })
}

/// The next power-of-two mebibyte above `total`, as a `warn_over` value.
fn suggest(total: usize) -> String {
    let mebibytes = total.div_ceil(1024 * 1024).max(1);
    format!("{}MiB", mebibytes.next_power_of_two())
}

#[cfg(test)]
mod tests;
