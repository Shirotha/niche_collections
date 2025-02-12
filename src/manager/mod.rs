mod exclusive;
pub use exclusive::*;

mod version;
pub use version::*;

use thiserror::Error;

use crate::StoreError;

#[derive(Debug, Error, PartialEq, Eq)]
enum ManagerError {
    #[error("store error: {0}")]
    StoreError(#[from] StoreError),
    #[error("bad handle {0}")]
    BadHandle(&'static str),
}
