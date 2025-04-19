mod exclusive;
pub use exclusive::*;
mod version;
use thiserror::Error;
pub use version::*;

use crate::StoreError;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ManagerError {
    #[error("store error: {0}")]
    StoreError(#[from] StoreError),
    #[error("bad handle {0}")]
    BadHandle(&'static str),
}
