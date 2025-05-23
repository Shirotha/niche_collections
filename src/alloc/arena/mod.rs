mod exclusive;
pub use exclusive::*;

mod version;
use thiserror::Error;
pub use version::*;

use super::*;
use crate::alloc::manager::ManagerError;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ArenaError {
    #[error("manager error: {0}")]
    ManagerError(#[from] ManagerError),
}
