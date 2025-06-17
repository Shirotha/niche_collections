pub mod alloc;
pub mod tree;

pub(crate) mod internal {
    pub trait Sealed {}
}

pub mod prelude {
    pub use crate::alloc::prelude::*;
}
