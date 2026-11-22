//! The collision rule, checked as a router is built for the pairs no type
//! carried both of: what an operation holds itself, against every scope
//! enclosing it.

use super::install::{error_at, path_location};
use super::{Router, RouterError, Violation};

impl<C, P, I, S> Router<C, P, I, S> {
    /// Refuses an interceptor an operation holds itself that collides with one
    /// a scope enclosing it applies, by the rule the compile-time check
    /// enforces wherever both types meet in one list.
    pub(super) fn refuse_colliding_interceptors(&self, violations: &mut Vec<Violation<RouterError>>)
    where
        C: 'static,
    {
        for mounted in &self.mounted {
            let held = mounted.endpoint.declared_interceptors();
            if held.is_empty() {
                continue;
            }

            let method = mounted.endpoint.method();
            let location = format!(
                "{}/{}",
                path_location(mounted.path.as_str()),
                method.as_wire_str().to_lowercase()
            );
            let scopes = self
                .interceptors
                .iter()
                .chain(&mounted.interceptors)
                .chain(&mounted.scoped_away);

            for scope in scopes {
                let scope = scope.declared();
                for endpoint in &held {
                    let Some(overlap) = endpoint.overlap(&scope) else {
                        continue;
                    };

                    let violation = error_at(
                        location.clone(),
                        RouterError::InterceptorsCollide {
                            endpoint: endpoint.name,
                            scope: scope.name,
                            overlap,
                        },
                    );
                    if !violations.contains(&violation) {
                        violations.push(violation);
                    }
                }
            }
        }
    }
}
