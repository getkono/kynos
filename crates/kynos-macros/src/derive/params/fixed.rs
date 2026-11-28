//! A header's `#[header(value = ...)]`: the one value its `()` field stands
//! for, sent, compared and described as its schema's `const`.

use proc_macro2::TokenStream as TokenStream2;

use super::Param;

/// The one value a fixed header carries, as its `#[header(value = ...)]`
/// wrote it. Either way it expands to a `&'static str` expression.
pub(crate) enum Fixed {
    /// A string literal, checked at expansion.
    Literal(syn::LitStr),
    /// A path to a `&'static str` constant, checked by a `const` assertion the
    /// derive emits, since only the compiler can read its value.
    Const(syn::Path),
}

impl quote::ToTokens for Fixed {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        match self {
            Self::Literal(literal) => literal.to_tokens(tokens),
            Self::Const(path) => path.to_tokens(tokens),
        }
    }
}

impl Param<'_> {
    /// Makes this a parameter that only ever carries `value`, or leaves it be.
    pub(crate) fn fixed(self, value: Option<Fixed>) -> Self {
        Self {
            fixed: value,
            ..self
        }
    }

    /// The one value this parameter carries, where it is fixed.
    pub(crate) fn fixed_value(&self) -> Option<&Fixed> {
        self.fixed.as_ref()
    }
}
