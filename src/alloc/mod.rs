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
    use super::manager::Kind;
    pub use super::{
        arena::{Guarded, Header, Headless, VArena, XArena, header},
        manager::{Mixed, Slices, Typed, VHandle, XHandle},
    };

    pub type SXArena<'id, K: Kind> = XArena<'id, K, K::SimpleStore<K::XElement>>;
    pub type RXArena<'id, K: Kind> = XArena<'id, K, K::ReuseableStore<K::XElement>>;

    pub type SVArena<'id, 'man, K: Kind, H = Headless> =
        VArena<'id, 'man, K, K::SimpleStore<K::VElement>, H>;
    pub type RVArena<'id, 'man, K: Kind, H = Headless> =
        VArena<'id, 'man, K, K::ReuseableStore<K::VElement>, H>;
}
