pub mod arena;
pub mod manager;
pub mod store;

pub type Index = nonmax::NonMaxU32;
pub type Length = u32;
pub type Version = std::num::NonZeroU32;

// TODO: implement serde (how to deal with handles and branded lifetimes)
// TODO: implement Index(Mut) for *Arena

#[allow(type_alias_bounds)]
pub mod prelude {
    pub use super::{
        arena::{Guarded, Header, Headless, VArena, XArena, header},
        manager::{Exclusive, Mixed, Slices, Typed, VHandle, Versioned, XHandle},
    };
}
