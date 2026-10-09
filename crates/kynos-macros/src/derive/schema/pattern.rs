//! `#[schema(pattern = "...")]`: an ECMA-262 regular expression, translated
//! by [`kynos_openapi::pattern`] when the derive expands, so a pattern no
//! check could enforce is a compile error rather than a panic on the first
//! request.

use super::{LitStr, TokenStream2, Type, quote};

/// The `pattern` check on `value`, a `&#ty`, against a `static` the block
/// declares, so the pattern compiles once per process however many values
/// reach it.
///
/// The static holds the pattern in the engine's dialect, translated here
/// rather than at run time. `refusals` has already refused one that does not
/// translate, so the empty expansion is never emitted.
pub(super) fn check(ty: &Type, declared: &LitStr) -> TokenStream2 {
    let Ok(translated) = translate(&declared.value()) else {
        return TokenStream2::new();
    };
    quote! {
        {
            static __KYNOS_PATTERN: ::kynos::__private::constraints::pattern::Pattern =
                ::kynos::__private::constraints::pattern::Pattern::new(#declared, #translated);
            ::kynos::__private::constraints::pattern::check::<#ty>(
                value,
                &__KYNOS_PATTERN,
                at,
                violations,
            );
        }
    }
}

/// `source` as the pattern the engine runs, or why it cannot be one.
pub(super) fn translate(source: &str) -> Result<String, String> {
    kynos_openapi::pattern::translate(source).map_err(|refusal| refusal.to_string())
}
