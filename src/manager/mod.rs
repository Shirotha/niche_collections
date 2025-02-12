mod exclusive;
pub use exclusive::*;
use thiserror::Error;

use crate::StoreError;

#[derive(Debug, Error, PartialEq, Eq)]
enum ManagerError {
    #[error("store error: {0}")]
    StoreError(#[from] StoreError),
}
