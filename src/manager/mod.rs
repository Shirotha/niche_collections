mod exclusive;
pub use exclusive::*;
mod version;
pub use version::*;

use crate::StoreError;
use thiserror::Error;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ManagerError {
    #[error("store error: {0}")]
    StoreError(#[from] StoreError),
    #[error("bad handle {0}")]
    BadHandle(&'static str),
}
