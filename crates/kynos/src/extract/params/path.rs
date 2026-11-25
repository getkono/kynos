//! Variables captured from the path template.

mod binding;
pub(crate) mod captures;

use std::marker::PhantomData;

use crate::{
    error::rejection::PathRejection,
    extract::{FromRequestParts, describe::Describe},
    http::Parts,
    router::operation::OperationCx,
    schema::{ParamValue, registry::Registry},
};

/// Variables captured from the path template.
///
/// `T` is a [`PathInput`]: a group deriving `PathParams`, or one
/// [`ParamValue`] for a route whose template declares one variable.
///
/// A group's wire names, each a field's `rename` or else its name under any
/// `rename_all` as the `Schema` derive applies it, are checked in order
/// against the route template's variables at compile time — a mismatch is a
/// compile error, not a runtime 500. One value takes the template's lone
/// variable, whatever it is called, and a template declaring any other number
/// is the same compile error:
///
/// ```
/// # #[cfg(feature = "macros")]
/// # {
/// use kynos::extract::params::path::Path;
///
/// #[kynos::delete("/sessions/{id}")]
/// async fn delete_session(Path(id): Path<u64>) {
///     let _ = id;
/// }
/// # }
/// ```
///
/// # Where the values come from
///
/// Each value the router captured is percent-decoded before it reaches
/// [`DecodePath::decode`], or one value's `FromStr`, so a variable holding
/// `%2F` arrives as `/`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Path<T>(pub T);

/// A group of path parameters, as the description sees it.
///
/// [`NAMES`](PathParams::NAMES) is what the route attribute compares against
/// the path template. A typed URI needs only [`EncodePath`]; an extracted
/// group needs only [`DecodePath`].
pub trait PathParams: Sized {
    /// The parameter names, in declaration order.
    const NAMES: &'static [&'static str];

    /// Describes each captured value as an OpenAPI path parameter.
    ///
    /// The default describes the declared [`NAMES`](PathParams::NAMES) with an
    /// unconstrained schema, and leaves `style` at its `simple` default.
    fn parameters(registry: &mut Registry) -> Vec<kynos_openapi::Parameter> {
        let _ = registry;
        Self::NAMES
            .iter()
            .copied()
            .map(|name| kynos_openapi::Parameter::path(name, kynos_openapi::Schema::any()))
            .collect()
    }

    /// The problems a value breaking a bound answers with, beside
    /// `about:blank`: the one the group names, else the
    /// [`CONSTRAINT_PROBLEM`](crate::schema::Schema::CONSTRAINT_PROBLEM) of
    /// each field type naming one, so what [`Path`] declares names what
    /// [`PathRejection::Schema`] sends.
    ///
    /// `#[derive(PathParams)]` replaces this; the default names none, which is
    /// right for a group holding no value to a bound.
    #[must_use]
    fn constraint_problems() -> Vec<crate::schema::constraints::ConstraintProblem> {
        Vec::new()
    }
}

/// Reading a path parameter group from a matched route.
///
/// `#[derive(PathParams)]` writes this, and the group's [`PathInput`]. Implement
/// it by hand only for a group that is extracted; one that only ever appears in
/// a typed URI implements [`EncodePath`] instead.
///
/// A group that encodes but does not decode cannot be extracted:
///
/// ```compile_fail
/// # use kynos::extract::params::path::{EncodePath, Named, Path, PathInput, PathParams};
/// struct Report;
///
/// impl PathParams for Report {
///     const NAMES: &'static [&'static str] = &["name"];
/// }
///
/// impl PathInput for Report {
///     type Binding = Named<Self>;
/// }
///
/// impl EncodePath for Report {
///     fn encode(&self) -> Vec<(&'static str, String)> {
///         vec![("name", "annual".to_owned())]
///     }
/// }
///
/// fn extracted<T: kynos::extract::FromRequestParts<()>>() {}
/// extracted::<Path<Report>>();
/// ```
///
/// The same group with a decoder, which is what a derive writes:
///
/// ```
/// # use kynos::{
/// #     error::rejection::PathRejection,
/// #     extract::params::path::{DecodePath, Named, Path, PathInput, PathParams},
/// # };
/// struct Report;
///
/// impl PathParams for Report {
///     const NAMES: &'static [&'static str] = &["name"];
/// }
///
/// impl PathInput for Report {
///     type Binding = Named<Self>;
/// }
///
/// impl DecodePath for Report {
///     fn decode(_: &[(&str, &str)]) -> Result<Self, PathRejection> {
///         Ok(Self)
///     }
/// }
///
/// fn extracted<T: kynos::extract::FromRequestParts<()>>() {}
/// extracted::<Path<Report>>();
/// ```
pub trait DecodePath: PathParams {
    /// Decodes the named captures from a matched route.
    fn decode(values: &[(&str, &str)]) -> Result<Self, PathRejection>;
}

/// Writing a path parameter group into a typed endpoint URI.
///
/// The counterpart to [`DecodePath`].
pub trait EncodePath: PathParams {
    /// Encodes this value for a typed endpoint URI.
    fn encode(&self) -> Vec<(&'static str, String)>;
}

/// What a [`Path`] extracts: a group of named variables, or one value.
///
/// `#[derive(PathParams)]` implements it for a group, binding it [`Named`],
/// and every [`ParamValue`] implements it, bound [`Lone`]: `Path<Uuid>` reads
/// the one variable its route's template declares, whatever that variable is
/// called, and the description names the parameter after it. A route
/// attribute refuses a template declaring any other number of variables at
/// compile time, and a router refuses an operation it was handed by
/// [`EndpointBuilder`](crate::router::endpoint::builder::EndpointBuilder)
/// that declares one.
///
/// A group implemented by hand writes what the derive does:
///
/// ```
/// # use kynos::{
/// #     error::rejection::PathRejection,
/// #     extract::params::path::{DecodePath, Named, PathInput, PathParams},
/// # };
/// struct Report;
/// # impl PathParams for Report {
/// #     const NAMES: &'static [&'static str] = &["name"];
/// # }
/// # impl DecodePath for Report {
/// #     fn decode(_: &[(&str, &str)]) -> Result<Self, PathRejection> {
/// #         Ok(Self)
/// #     }
/// # }
///
/// impl PathInput for Report {
///     type Binding = Named<Self>;
/// }
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` is neither a path parameter group nor one path value",
    label = "not extractable from the path",
    note = "`Path<T>` takes a group deriving `kynos::PathParams`, whose fields name the \
            template's variables, or one `kynos::schema::ParamValue`, which reads a template's \
            lone variable"
)]
pub trait PathInput: Sized {
    /// How the value binds its route's variables: [`Named<Self>`] for a
    /// group, [`Lone<Self>`] for one value.
    type Binding;
}

/// The binding of a group, each of whose [`PathParams::NAMES`] reads the
/// variable of that name.
#[derive(Debug)]
pub struct Named<T>(PhantomData<fn() -> T>);

/// The binding of one [`ParamValue`], which reads the one variable its
/// operation's template declares.
///
/// The parameter is described under that variable's name, required as every
/// path parameter is, with `T`'s schema and no description of its own: a
/// doc comment on `T` describes the schema, and a group's field documents
/// the parameter itself.
#[derive(Debug)]
pub struct Lone<T>(PhantomData<fn() -> T>);

impl<T: ParamValue> PathInput for T {
    type Binding = Lone<T>;
}

mod private {
    pub trait Sealed {}
}

impl<T> private::Sealed for Named<T> {}
impl<T> private::Sealed for Lone<T> {}

/// What a [`PathInput`]'s binding does to read and describe `T`: implemented
/// by [`Named`] over a [`DecodePath`] group and by [`Lone`], and by nothing
/// else.
pub trait PathBinding<T>: private::Sealed {
    /// The variables `T` names, in order, which the route attribute checks
    /// against its template; `None` for the template's one variable, whatever
    /// it is called.
    const NAMES: Option<&'static [&'static str]>;

    /// Describes each parameter `T` reads, `variables` being the operation's
    /// own template's, before any prefix.
    fn parameters(registry: &mut Registry, variables: &[String]) -> Vec<kynos_openapi::Parameter>;

    /// The problems a value breaking a bound answers with, beside
    /// `about:blank`.
    fn constraint_problems() -> Vec<crate::schema::constraints::ConstraintProblem>;

    /// Reads `T` from what the matched route captured.
    ///
    /// # Errors
    ///
    /// Rejects a variable the match did not capture, one that does not
    /// percent-decode to UTF-8, and a value `T` does not read or whose bounds
    /// it breaks.
    fn decode(parts: &Parts) -> Result<T, PathRejection>;
}

/// What a [`PathInput`]'s binding does to write `T` into a typed URI:
/// implemented by [`Named`] over an [`EncodePath`] group and by [`Lone`].
pub trait EncodeBinding<T>: private::Sealed {
    /// Each variable of a template declaring `variables` paired with the text
    /// `value` fills it with, not yet percent-encoded.
    fn encode(value: &T, variables: &[&'static str]) -> Vec<(&'static str, String)>;
}

impl<C: Sync, T: PathInput<Binding: PathBinding<T>> + Send> FromRequestParts<C> for Path<T> {
    type Rejection = PathRejection;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        T::Binding::decode(parts).map(Path)
    }
}

impl<T: PathInput<Binding: PathBinding<T>>> Describe for Path<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let variables = operation.path_variables();
        let parameters = T::Binding::parameters(operation.registry(), variables);
        for parameter in parameters {
            operation.add_parameter(parameter);
        }
        super::describe_constraints(operation, &T::Binding::constraint_problems());
    }
}

#[cfg(test)]
mod tests;
