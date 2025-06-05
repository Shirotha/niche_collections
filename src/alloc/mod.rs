pub mod arena;
pub mod manager;
pub mod store;

pub type Index = nonmax::NonMaxU32;
pub type Length = u32;
pub type Version = std::num::NonZeroU32;

#[allow(type_alias_bounds)]
pub mod prelude {
    use super::store::SimpleStore;
    pub use super::{
        arena::{Guarded, Headless, VArena, XArena, header},
        manager::{Kind, Mixed, Slices, Typed, VHandle, XHandle},
    };

    pub type SXArena<'id, K: Kind> = XArena<'id, K, SimpleStore<K::XElement>>;
    pub type RXArena<'id, K: Kind> = XArena<'id, K, K::ReuseableStore<K::XElement>>;

    pub type SVArena<'id, 'man, K: Kind, H> = VArena<'id, 'man, K, SimpleStore<K::VElement>, H>;
    pub type RVArena<'id, 'man, K: Kind, H> =
        VArena<'id, 'man, K, K::ReuseableStore<K::VElement>, H>;
}
