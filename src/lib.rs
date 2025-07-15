pub mod alloc;

pub(crate) mod internal {
    pub trait Sealed {}
}

pub mod prelude {
    pub use crate::alloc::prelude::*;
}
