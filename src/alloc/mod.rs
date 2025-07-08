use std::num::NonZeroU32;

pub mod arena;
pub mod manager;
pub mod store;

pub type Index = nonmax::NonMaxU32;
pub type Length = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Version(NonZeroU32);
impl Version {
    const fn new(value: u32) -> Option<Self> {
        match NonZeroU32::new(value) {
            Some(version) => Some(Self(version)),
            None => None,
        }
    }
    const fn checked_add(self, other: u32) -> Option<Self> {
        match self.0.checked_add(other) {
            Some(result) => Some(Self(result)),
            None => None,
        }
    }
}

// TODO: implement serde (how to deal with handles and branded lifetimes)
// TODO: implement Index(Mut) for *Arena

#[allow(type_alias_bounds)]
pub mod prelude {
    pub use super::{
        arena::{Arena, Guarded, Header, Headless, header},
        manager::{Exclusive, Mixed, Slices, SoA, Typed, VHandle, Versioned, XHandle},
    };
}
