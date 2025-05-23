pub mod arena;
pub mod manager;
pub mod store;

pub type Index = nonmax::NonMaxU32;
pub type Length = u32;
pub type Version = std::num::NonZeroU32;
