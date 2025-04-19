mod exclusive;
pub use exclusive::*;

mod version;
use thiserror::Error;
pub use version::*;

use crate::ManagerError;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ArenaError {
    #[error("manager error: {0}")]
    ManagerError(#[from] ManagerError),
}
