mod exclusive;
pub use exclusive::*;

mod version;
use thiserror::Error;
pub use version::*;

use super::{manager::*, *};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ArenaError {
    #[error("manager error: {0}")]
    ManagerError(#[from] ManagerError),
}
pub type AResult<T> = Result<T, ArenaError>;
pub struct Arena<'id, 'man, K, C>(<GlobalConfig<K, C> as Config>::Arena<'id, 'man>)
where
    GlobalConfig<K, C>: Config;
