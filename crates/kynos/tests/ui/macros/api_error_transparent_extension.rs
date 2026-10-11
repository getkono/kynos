//! A transparent variant publishes the extension members of the error it
//! holds; the field holding it is not one.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("no user with that id")]
#[problem(status = 404)]
struct LookupError;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error(transparent)]
    #[problem(transparent)]
    Lookup {
        #[problem(extension)]
        #[from]
        source: LookupError,
    },
}

fn main() {}
