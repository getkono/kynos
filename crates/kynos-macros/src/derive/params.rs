//! What the four parameter derives share: decoding and encoding a field here,
//! describing it in [`describe`]; each derive supplies only its own lookup.
//!
//! A value becomes a field through [`FromStr`](std::str::FromStr) alone, never
//! serde. Each field's type is bounded by `kynos::schema::ParamValue`, so its
//! schema describes the one value `FromStr` reads. An `Option<T>` field is
//! optional and described by `T`'s schema; the recognition is syntactic, so an
//! alias for it reads as required. A query group's `Vec<T>` field is a
//! [`List`] of `T`s, recognised the same way.

pub(crate) mod describe;

use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use syn::{DeriveInput, Expr, Field, FieldsNamed, Ident, Type, spanned::Spanned};

use crate::derive::{
    common::wrapped,
    schema::{check::value::value_checks, group_attributes},
};

/// The problem a group's own `#[schema(constraint_problem = ...)]` names,
/// which answers every field's broken bound in place of its type's: the value
/// holding a bound answers for it, as a body's type does for its members, so
/// a group reads alike through its parameter extractor and `QueryString`.
///
/// Read as the `Schema` derive reads it, refusing a malformed `#[schema(...)]`
/// on the group or a field as that derive does, since each parameter derive
/// registers the attribute and may stand without it.
pub(crate) fn group_problem(input: &DeriveInput) -> syn::Result<Option<Expr>> {
    group_attributes(input)
}

/// Where the problem a group answers every field's broken bound with comes from.
#[derive(Clone, Copy)]
pub(crate) enum Group<'a> {
    /// None: each field answers with its type's.
    None,
    /// The [`group_problem`] its attribute names.
    Named(&'a Expr),
}

impl<'a> From<Option<&'a Expr>> for Group<'a> {
    fn from(problem: Option<&'a Expr>) -> Self {
        problem.map_or(Self::None, Self::Named)
    }
}

/// One field of a parameter group, paired with the wire name it occupies.
pub(crate) struct Param<'a> {
    field: &'a Field,
    name: String,
    /// The one value a header's `()` field stands for, as its schema's `const`.
    fixed: Option<String>,
    /// Where the group's problem for every field's broken bound comes from.
    group: Group<'a>,
    /// How a query group's `Vec<T>` field carries its items, where it is one.
    list: Option<List>,
}

/// How a list parameter carries its items: OpenAPI's `style: form` with
/// `explode` either way, the only two forms a query list has under RFC 6570.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum List {
    /// One pair per item, `tag=a&tag=b`: `explode: true`.
    Exploded,
    /// One pair, its items joined by literal commas, `tag=a,b`:
    /// `explode: false`.
    Delimited,
}

impl<'a> Param<'a> {
    /// Pairs each field with the wire name already resolved for it, under the
    /// problem `group` names for the whole group.
    pub(crate) fn pair(
        fields: &'a FieldsNamed,
        names: &[String],
        group: impl Into<Group<'a>>,
    ) -> Vec<Self> {
        let group = group.into();
        fields
            .named
            .iter()
            .zip(names)
            .map(|(field, name)| Self {
                field,
                name: name.clone(),
                fixed: None,
                group,
                list: None,
            })
            .collect()
    }

    /// Makes this a list parameter carried as `list` says, or leaves it be.
    pub(crate) fn listed(self, list: Option<List>) -> Self {
        Self { list, ..self }
    }

    /// How this parameter carries its items, where it is a list.
    pub(crate) fn list(&self) -> Option<List> {
        self.list
    }

    /// The field as written, for reading its attributes.
    pub(crate) fn field(&self) -> &Field {
        self.field
    }

    /// Makes this a parameter that only ever carries `value`, or leaves it be.
    pub(crate) fn fixed(self, value: Option<String>) -> Self {
        Self {
            fixed: value,
            ..self
        }
    }

    /// The one value this parameter carries, where it is fixed.
    pub(crate) fn fixed_value(&self) -> Option<&str> {
        self.fixed.as_deref()
    }

    /// The name this parameter is carried under.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The identifier the struct literal binds.
    pub(crate) fn ident(&self) -> &Ident {
        self.field
            .ident
            .as_ref()
            .expect("a parameter group is a struct with named fields")
    }

    pub(crate) fn ty(&self) -> &Type {
        &self.field.ty
    }

    /// The `T` of an `Option<T>` field, which is what makes it optional.
    pub(crate) fn optional(&self) -> Option<&Type> {
        wrapped(self.ty(), "Option")
    }

    /// The one value's type: a list's item, an `Option`'s inner type, else
    /// the field's own. What `ParamValue` bounds, and whose problem a broken
    /// bound answers with.
    fn carried(&self) -> &Type {
        if self.list.is_some() {
            return wrapped(self.ty(), "Vec").expect("only a `Vec` field is listed");
        }
        self.optional().unwrap_or(self.ty())
    }
}

/// Binds one field from an already-located value, or returns a rejection.
///
/// `found` is an expression of type `Option<&str>`, `rejection` the type the
/// derive's `decode` returns, and `missing` what a required parameter says when
/// nothing carried it. The `ParamValue` assertion is spanned at the field and
/// lives here because every derive's `decode` calls this.
pub(crate) fn decode_field(
    param: &Param<'_>,
    rejection: &TokenStream2,
    found: &TokenStream2,
    missing: &str,
) -> TokenStream2 {
    let ident = param.ident();
    let ty = param.ty();
    let name = &param.name;
    let bounded = bounded(param);

    let read = if let Some(inner) = param.optional() {
        let parsed = parse(param, inner, rejection, None);
        quote! {
            match found {
                ::core::option::Option::Some(raw) => ::core::option::Option::Some(#parsed),
                ::core::option::Option::None => ::core::option::Option::None,
            }
        }
    } else {
        let parsed = parse(param, ty, rejection, None);
        quote! {
            match found {
                ::core::option::Option::Some(raw) => #parsed,
                ::core::option::Option::None => {
                    return ::core::result::Result::Err(#rejection::Invalid {
                        name: <::std::string::String as ::core::convert::From<_>>::from(#name),
                        detail: <::std::string::String as ::core::convert::From<_>>::from(#missing),
                    });
                }
            }
        }
    };

    quote! {
        #bounded
        let #ident: #ty = {
            let found: ::core::option::Option<&str> = #found;
            #read
        };
    }
}

/// Binds a list field from its items' octets, or returns a rejection naming
/// the parameter and the index of the item that failed.
///
/// `items` is an expression iterating `Cow<'_, [u8]>`, one per item in wire
/// order; none at all binds the empty list. An item is refused where its
/// octets are not UTF-8 or its text does not parse, as one value is.
pub(crate) fn decode_list(
    param: &Param<'_>,
    rejection: &TokenStream2,
    items: &TokenStream2,
) -> TokenStream2 {
    let ident = param.ident();
    let ty = param.ty();
    let name = &param.name;
    let bounded = bounded(param);
    let index = Ident::new("index", proc_macro2::Span::call_site());
    let parsed = parse(param, param.carried(), rejection, Some(&index));

    quote! {
        #bounded
        let #ident: #ty = {
            let mut list = ::std::vec::Vec::new();
            for (#index, octets) in ::core::iter::Iterator::enumerate(
                ::core::iter::IntoIterator::into_iter(#items),
            ) {
                let raw = match ::core::str::from_utf8(&octets) {
                    ::core::result::Result::Ok(text) => text,
                    ::core::result::Result::Err(_) => {
                        return ::core::result::Result::Err(#rejection::Invalid {
                            name: <::std::string::String as ::core::convert::From<_>>::from(#name),
                            detail: ::std::format!(
                                "item {}: the percent-decoded value is not UTF-8",
                                #index,
                            ),
                        });
                    }
                };
                list.push(#parsed);
            }
            list
        };
    }
}

/// The assertion that the field carries `ParamValue`s, spanned at the type it
/// bounds: an `Option<T>`'s or a list's `T`, else the field's.
fn bounded(param: &Param<'_>) -> TokenStream2 {
    let carried = param.carried();
    quote_spanned! {carried.span()=>
        {
            fn carried_by_one_parameter<T: ::kynos::schema::ParamValue>() {}
            carried_by_one_parameter::<#carried>();
        }
    }
}

/// Parses `raw`, a `&str` in scope, as `inner`, else returns `Invalid` naming
/// the parameter with the error's text, prefixed with the item's index where
/// `index` names the binding holding it.
fn parse(
    param: &Param<'_>,
    inner: &Type,
    rejection: &TokenStream2,
    index: Option<&Ident>,
) -> TokenStream2 {
    let name = &param.name;
    let span = param.field.ty.span();

    // Only the paths are spanned to the field, so a type that does not parse
    // or whose error does not display is reported there; `raw` keeps the
    // derive's hygiene context, wherever the type's tokens came from.
    let to_string = quote_spanned!(span=> ::std::string::ToString::to_string);
    let from_str = quote_spanned!(span=> <#inner as ::core::str::FromStr>::from_str);
    let detail = index.map_or_else(
        || quote!(#to_string(&error)),
        |index| quote!(::std::format!("item {}: {}", #index, #to_string(&error))),
    );
    quote! {
        match #from_str(raw) {
            ::core::result::Result::Ok(value) => value,
            ::core::result::Result::Err(error) => {
                return ::core::result::Result::Err(#rejection::Invalid {
                    name: <::std::string::String as ::core::convert::From<_>>::from(#name),
                    detail: #detail,
                });
            }
        }
    }
}

/// The problem a broken bound on the field answers with: the group's, else
/// the `CONSTRAINT_PROBLEM` of the `ParamValue` it carries.
fn carried(param: &Param<'_>) -> TokenStream2 {
    let typed = typed(param);
    match param.group {
        Group::None => typed,
        Group::Named(group) => quote!(::core::option::Option::Some(#group)),
    }
}

/// The `CONSTRAINT_PROBLEM` of the `ParamValue` the field carries: a list's
/// item's, since `Vec` names none of its own.
fn typed(param: &Param<'_>) -> TokenStream2 {
    let carried = param.carried();
    quote!(<#carried as ::kynos::schema::Schema>::CONSTRAINT_PROBLEM)
}

/// The `constraint_problems` body: the group's problem once where it names
/// one and holds a checked field, else each checked field's type's
/// `CONSTRAINT_PROBLEM`, in declaration order, those naming none left out. A
/// fixed field carries no value to check.
pub(crate) fn constraint_problems_body(params: &[Param<'_>]) -> TokenStream2 {
    let group = params.first().map_or(Group::None, |param| param.group);
    let checked = params.iter().filter(|param| param.fixed.is_none());
    let named: Vec<TokenStream2> = match group {
        Group::Named(_) => checked.take(1).map(carried).collect(),
        Group::None => checked.map(typed).collect(),
    };
    let count = named.len();
    quote! {
        // Typed, so a group with no checked field still names its element.
        let named: [
            ::core::option::Option<::kynos::schema::constraints::ConstraintProblem>;
            #count
        ] = [#(#named),*];
        ::core::iter::Iterator::collect(::core::iter::Iterator::flatten(
            ::core::iter::IntoIterator::into_iter(named),
        ))
    }
}

/// Refuses a decoded field that breaks one of its `#[schema(...)]` bounds, or
/// one its type declares, as `Schema`, naming the parameter, each failure at a
/// pointer into its value, and carrying the problem the group names, else the
/// field's type. Runs after [`decode_field`] has bound the field.
pub(crate) fn check_field(param: &Param<'_>, rejection: &TokenStream2) -> TokenStream2 {
    let ident = param.ident();
    let ty = param.ty();
    let name = &param.name;
    let checks = value_checks(param.field);
    let problem = carried(param);

    quote! {
        {
            let value: &#ty = &#ident;
            let at = ::kynos::schema::constraints::Pointer::root();
            let mut __kynos_violations = ::kynos::schema::constraints::Violations::new();
            {
                let violations = &mut __kynos_violations;
                #checks
            }
            if !__kynos_violations.is_empty() {
                return ::core::result::Result::Err(#rejection::Schema {
                    name: <::std::string::String as ::core::convert::From<_>>::from(#name),
                    failures: __kynos_violations.into_failures(),
                    problem: #problem,
                });
            }
        }
    }
}

/// The struct literal a `decode` body ends with.
pub(crate) fn construct(params: &[Param<'_>]) -> TokenStream2 {
    let idents = params.iter().map(Param::ident);
    quote!(::core::result::Result::Ok(Self { #(#idents),* }))
}

/// This field's value as an `Option<String>`, absent only when the field is.
pub(crate) fn render(param: &Param<'_>) -> TokenStream2 {
    let ident = param.ident();
    // Only the path is spanned to the field, so a type that does not display
    // is reported there; `self` keeps the derive's hygiene context.
    let to_string = quote_spanned!(param.field.ty.span()=> ::std::string::ToString::to_string);

    if param.optional().is_some() {
        quote! {
            ::core::option::Option::map(::core::option::Option::as_ref(&self.#ident), #to_string)
        }
    } else {
        quote! {
            ::core::option::Option::Some(#to_string(&self.#ident))
        }
    }
}
