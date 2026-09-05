//! What a request can reach through the dispatch table.
//!
//! `docs/performance.md` grades the emitted document, the schema registry and
//! the document validators as off-path elements, and an off-path element owes a
//! proof that a request cannot reach it. The naming rule in
//! `scripts/containment.py` holds one half of that proof: each element is named
//! only at the sites `docs/testing.md#the-off-path-proof` allows it, and
//! nothing under `router/` that a request runs through is one of them.
//!
//! A scan cannot hold the other half. The table hands every request to a trait
//! object -- `dyn ErasedTerminal`, `dyn ErasedInterceptor`, `dyn Observer`,
//! `dyn ErasedLayer` -- and what sits behind one of those is declared in
//! another file, where the naming rule sees an allowed site rather than the
//! request path. The three witnesses below close it from the other side:
//! nothing reaches an erased callee that [`Dispatch`], [`PathEntry`] or
//! [`Served`] does not carry, so pinning their fields pins what a request can
//! reach.
//!
//! Each pattern is exhaustive, so a new field is a `missing field in pattern`
//! error here until whoever added it writes it into the pattern -- which is the
//! moment to argue that a request may reach it, rather than a later moment
//! nobody arrives at. The pattern is the assertion and the bodies are never
//! called, which is why each carries `#[allow(dead_code)]` rather than a
//! `#[test]` that would coerce it to a fn pointer and assert nothing:
//! `docs/testing.md#the-allocation` allocates a witness fn to a rule the
//! compiler checks, and says such a rule does not owe running.

use super::{Dispatch, PathEntry, Served};

/// Every field the whole route table carries.
#[allow(dead_code)]
fn dispatch_fields(dispatch: &Dispatch<()>) {
    let Dispatch {
        matcher,
        paths,
        context,
        observers,
        not_found,
        method_not_allowed,
        trailing_slashes,
        trusted_proxies,
    } = dispatch;

    let _ = (
        matcher,
        paths,
        context,
        observers,
        not_found,
        method_not_allowed,
        trailing_slashes,
        trusted_proxies,
    );
}

/// Every field one `paths` key carries into the request that matched it.
#[allow(dead_code)]
fn path_entry_fields(entry: &PathEntry<()>) {
    let PathEntry {
        template,
        matched,
        variables,
        allow,
        operations,
    } = entry;

    let _ = (template, matched, variables, allow, operations);
}

/// Every field one declared operation carries into the chain that serves it.
///
/// `unchecked_layers` is bound under the gate that declares it, so the pattern
/// is exhaustive at baseline features as well as under `--all-features`: a
/// field only one of the two builds checks is a field the other build would not
/// notice arriving.
#[allow(dead_code)]
fn served_fields(served: &Served<()>) {
    let Served {
        method,
        operation_id,
        terminal,
        interceptors,
        catch_panics,
        #[cfg(feature = "unchecked")]
        unchecked_layers,
    } = served;

    #[cfg(feature = "unchecked")]
    let _ = unchecked_layers;

    let _ = (method, operation_id, terminal, interceptors, catch_panics);
}
