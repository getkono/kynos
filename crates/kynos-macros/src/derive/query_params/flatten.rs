//! A `#[param(flatten)]` field: another query group whose parameters are
//! this group's, decoded, described and encoded by that group itself.
//!
//! The flattened group reads the whole query string again, so each name
//! stays owned by the one group declaring it: its own repeated pair refusal
//! and its own bounds hold unchanged, and a group flattening another that
//! flattens a third needs nothing more. Its names are known only to its own
//! `parameters`, so a name it shares with this group, or with another
//! flattened group, is refused when the router is built.

use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use syn::{
    DeriveInput, Expr, Field, FieldsNamed, Generics, Ident, Type, WhereClause, WherePredicate,
    spanned::Spanned,
};

use crate::derive::{
    attribute::{FieldArgs, field_args},
    common::{NameCase, reject_duplicate_names, wire_names, wrapped},
    params::{Param, describe::collected},
};

/// One field of the group, in declaration order.
pub(crate) enum Member<'a> {
    /// One parameter, the `index`th of the group's own.
    Own(usize),
    /// A flattened group's parameters.
    Flattened(Flattened<'a>),
}

/// The group's own parameters, and every field as a [`Member`] in
/// declaration order, each own parameter's name checked against the others'.
pub(crate) fn members<'a>(
    input: &DeriveInput,
    fields: &'a FieldsNamed,
    group: Option<&'a Expr>,
) -> syn::Result<(Vec<Param<'a>>, Vec<Member<'a>>)> {
    let names = wire_names(input, fields, "param")?;
    let (mut own, mut members) = (Vec::new(), Vec::new());
    for (field, param) in fields.named.iter().zip(Param::pair(fields, &names, group)) {
        let args = field_args(field, "param")?;
        if let Some(flattened) = Flattened::new(field, &args)? {
            members.push(Member::Flattened(flattened));
            continue;
        }
        let list = super::list(&param, args.explode)?;
        let default = super::default(&param, args.default)?;
        members.push(Member::Own(own.len()));
        own.push(param.listed(list).defaulted(default));
    }

    // A flattened field names no parameter of its own; its group's names
    // are checked when the router is built.
    let mut declared = fields.clone();
    declared.named = own.iter().map(|param| param.field().clone()).collect();
    let own_names: Vec<String> = own.iter().map(|param| param.name().to_owned()).collect();
    reject_duplicate_names(
        &declared,
        &own_names,
        "query parameter",
        NameCase::Sensitive,
    )?;
    Ok((own, members))
}

/// The group's where clause for its `trait_name` impl, with each flattened
/// field's type bounded by the same trait, which the impl delegates to.
pub(crate) fn bounded(
    generics: &Generics,
    flattened: &[&Flattened<'_>],
    trait_name: &str,
) -> WhereClause {
    let mut generics = generics.clone();
    let clause = generics.make_where_clause();
    clause.predicates.extend(
        flattened
            .iter()
            .map(|flattened| flattened.bound(trait_name)),
    );
    clause.clone()
}

/// The `parameters` and `constraint_problems` bodies of a group flattening
/// any: `entries`, one per member, each recording its range of `parameters`
/// so a name two members share is recorded on `registry`; and the problems
/// `own` names, joined by each flattened group's not already among them.
pub(crate) fn composed(
    entries: Vec<TokenStream2>,
    own: &TokenStream2,
    flattened: &[&Flattened<'_>],
) -> (TokenStream2, TokenStream2) {
    let entries = entries.into_iter().map(|entry| {
        quote! {
            let start = parameters.len();
            #entry
            members.push(start..parameters.len());
        }
    });
    let parameters = collected(
        std::iter::once(quote!(let mut members = ::std::vec::Vec::new();)).chain(entries),
        &quote! {
            ::kynos::__private::query::refuse_repeated(
                registry,
                ::core::any::type_name::<Self>(),
                &parameters,
                &members,
            );
        },
    );
    let joined = flattened
        .iter()
        .map(|flattened| flattened.constraint_problems());
    let problems = quote! {
        let mut problems: ::std::vec::Vec<::kynos::schema::constraints::ConstraintProblem> =
            { #own };
        #(#joined)*
        problems
    };
    (parameters, problems)
}

/// A field holding a flattened query group.
pub(crate) struct Flattened<'a> {
    field: &'a Field,
}

impl<'a> Flattened<'a> {
    /// The field, where its `#[param(...)]` says `flatten`.
    ///
    /// Refuses every other `#[param]` key beside it, and a `#[schema(...)]`
    /// on it, since the group's own fields say each parameter's name, default,
    /// carriage and bounds; and an `Option` around the group, which is never
    /// absent: its own fields say which of its parameters may be.
    pub(crate) fn new(field: &'a Field, args: &FieldArgs) -> syn::Result<Option<Self>> {
        if args.flatten.is_none() {
            return Ok(None);
        }
        let beside = [
            args.rename.as_ref().map(|key| ("rename", key.span())),
            args.default.as_ref().map(|key| ("default", key.span())),
            args.explode.as_ref().map(|key| ("explode", key.span())),
        ];
        if let Some((key, span)) = beside.into_iter().flatten().next() {
            return Err(syn::Error::new(
                span,
                format!(
                    "a flattened group's parameters take their `{key}` from its own fields, so \
                     `{key}` beside `flatten` would say nothing. Write it on the field of the \
                     group it belongs to"
                ),
            ));
        }
        if let Some(attr) = field
            .attrs
            .iter()
            .find(|attr| attr.path().is_ident("schema"))
        {
            return Err(syn::Error::new(
                attr.span(),
                "a flattened group is no one value to bound, so `#[schema(...)]` beside \
                 `flatten` would bound nothing. Write each bound on the field of the group it \
                 belongs to",
            ));
        }
        if wrapped(&field.ty, "Option").is_some() {
            return Err(syn::Error::new(
                field.ty.span(),
                "a flattened group is never absent, since its own fields say which of its \
                 parameters may be, so an `Option` around it would never be `None`. Flatten the \
                 group itself",
            ));
        }
        Ok(Some(Self { field }))
    }

    /// The identifier the struct literal binds.
    pub(crate) fn ident(&self) -> &Ident {
        self.field
            .ident
            .as_ref()
            .expect("a parameter group is a struct with named fields")
    }

    fn ty(&self) -> &Type {
        &self.field.ty
    }

    /// The bound the group's `trait_name` impl needs of this field's type,
    /// spanned at the field so a type that is no query group is reported
    /// there.
    pub(crate) fn bound(&self, trait_name: &str) -> WherePredicate {
        let ty = self.ty();
        let trait_name = Ident::new(trait_name, ty.span());
        syn::parse_quote_spanned! {ty.span()=>
            #ty: ::kynos::extract::params::query::#trait_name
        }
    }

    /// Binds the field from the whole query string [`WHOLE`] holds, its
    /// rejection this group's.
    pub(crate) fn decode(&self) -> TokenStream2 {
        let ident = self.ident();
        let ty = self.ty();
        let whole = whole();
        quote! {
            let #ident: #ty =
                <#ty as ::kynos::extract::params::query::DecodeQuery>::decode(#whole)?;
        }
    }

    /// Pushes the group's own parameters onto `parameters`.
    pub(crate) fn parameters(&self) -> TokenStream2 {
        let ty = self.ty();
        quote! {
            ::core::iter::Extend::extend(
                &mut parameters,
                <#ty as ::kynos::extract::params::query::QueryParams>::parameters(registry),
            );
        }
    }

    /// Adds each problem the group answers a broken bound with to
    /// `problems`, once.
    pub(crate) fn constraint_problems(&self) -> TokenStream2 {
        let ty = self.ty();
        quote! {
            for problem in
                <#ty as ::kynos::extract::params::query::QueryParams>::constraint_problems()
            {
                if !problems.contains(&problem) {
                    problems.push(problem);
                }
            }
        }
    }

    /// Appends the group's own pairs to `query`, joined by an `&` where both
    /// hold any.
    pub(crate) fn encode(&self) -> TokenStream2 {
        let ident = self.ident();
        let ty = self.ty();
        let encode =
            quote_spanned!(ty.span()=> ::kynos::extract::params::query::EncodeQuery::encode);
        quote! {
            let flattened = #encode(&self.#ident);
            if !flattened.is_empty() {
                if !query.is_empty() {
                    query.push('&');
                }
                query.push_str(&flattened);
            }
        }
    }
}

/// The local `decode` binds the whole query string to before any field is
/// bound, so a field named `query` does not shadow it.
const WHOLE: &str = "__kynos_whole_query";

/// [`WHOLE`] as an identifier.
fn whole() -> Ident {
    Ident::new(WHOLE, proc_macro2::Span::call_site())
}

/// Binds [`WHOLE`] to `decode`'s argument, where the group flattens any.
pub(crate) fn keep_whole() -> TokenStream2 {
    let whole = whole();
    quote!(let #whole: ::core::option::Option<&str> = query;)
}
