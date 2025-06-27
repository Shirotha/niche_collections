mod collection;
mod guard;
mod handle;
use std::{cell::UnsafeCell, sync::Arc};

pub use collection::*;
use generativity::{Guard, Id};
pub use guard::*;
pub use handle::*;
use parking_lot::RwLock;

use super::*;
use crate::alloc::{manager::*, store::*};

type ManagerCell<'man, K, C> = UnsafeCell<Manager<'man, K, C>>;
type ArcLock<T> = Arc<RwLock<T>>;

// TODO: should there be on_insert, on_remove, on_move handlers?
pub trait Header: Default {
    #[expect(unused_variables)]
    fn merge(&mut self, other: &mut Self) {}
}
/// # Example
/// ```
/// use niche_collections::alloc::arena::header;
/// header! {
///     #[derive(Clone, Copy, PartialEq, Eq)]
///     pub SimpleHeader<T: Default>(_this: _ = T::default(), _other: T) {}
/// }
/// ```
#[macro_export]
macro_rules! header {
    {
        $( #[derive( $( $derive:ty ),* )] )?
        $visibility:vis $typename:ident $( <
            $( $param:tt $( : $( $constraint:path ),+ )? ),+
        > )? (
            $dataname:ident : _ = $datadefault:expr,
            $othername:ident : $datatype:ty
        ) $mergebody:block
    } => {
        $( #[derive( $( $derive ),* )] )?
        $visibility struct $typename $( <
            $( $param ),*
        > )? ( $datatype );
        impl $( <
            $( $param $( : $( $constraint ),* )? ),*
        > )? $crate::alloc::arena::Header for $typename $( <
            $( $param ),*
        > )? {
            fn merge(&mut self, other: &mut Self) {
                let $dataname = &mut self.0;
                let $othername = &other.0;
                $mergebody
            }
        }
        impl $( <
            $( $param $( : $( $constraint ),* )? ),*
        > )? core::default::Default for $typename $( <
            $( $param ),*
        > )? {
            fn default() -> Self {
                Self($datadefault)
            }
        }
    };
}
pub use header;
header! {
    #[derive(Debug, Clone, Copy)]
    pub Headless(_this: _ = (), _other: ()) {}
}

#[derive(Debug, Clone)]
pub struct VArena<'id, 'man, K, C, H: Header = Headless>
where
    GlobalConfig<K, C>: Config,
{
    manager: ArcLock<ManagerCell<'man, K, C>>,
    port:    ArcLock<(Id<'id>, H)>,
}
impl<'id, 'man, K, const REUSE: bool, H, V> Arena<'id, 'man, K, Versioned<REUSE, H, V>>
where
    H: Header,
    GlobalConfig<K, Versioned<REUSE, H, V>>: for<'x, 'y> Config<
            Manager<'x> = VManager<'x, K, Versioned<REUSE, H, V>>,
            Arena<'y, 'x> = VArena<'y, 'x, K, Versioned<REUSE, H, V>, H>,
        >,
{
    pub fn split<'new>(&self, guard: Guard<'new>) -> Arena<'new, 'man, K, Versioned<REUSE, H, V>> {
        Arena(VArena {
            manager: self.0.manager.clone(),
            port:    Arc::new(RwLock::new((guard.into(), H::default()))),
        })
    }
    /// # Safety
    /// After this call accessing `VHandle<'other, _>` causes undefined behaviour.
    /// This can deadlock if `self` is the same as `other`.
    /// Making this safe requires `H` to carry additional data and thus is not being done by default.
    pub unsafe fn join<'other>(
        &self,
        other: VArena<'other, 'man, K, Versioned<REUSE, H, V>, H>,
    ) -> HandleMap<'other, 'id> {
        let mut this_port = self.0.port.write();
        let mut other_port = other.port.write();
        this_port.1.merge(&mut other_port.1);
        HandleMap { _from: other_port.0, _to: this_port.0 }
    }
    pub fn read(&self) -> VArenaReadGuard<'_, 'id, 'man, K, Versioned<REUSE, H, V>, H> {
        VArenaReadGuard { manager: self.0.manager.read(), port: self.0.port.read() }
    }
    pub fn write(&mut self) -> VArenaWriteGuard<'_, 'id, 'man, K, Versioned<REUSE, H, V>, H> {
        VArenaWriteGuard { manager: self.0.manager.read(), port: self.0.port.write() }
    }
    pub fn alloc(&mut self) -> VArenaAllocGuard<'_, 'id, 'man, K, Versioned<REUSE, H, V>, H> {
        VArenaAllocGuard { manager: self.0.manager.upgradable_read(), port: self.0.port.write() }
    }
}
impl<'id, 'man, K, const REUSE: bool, H, V> Arena<'id, 'man, K, Versioned<REUSE, H, V>>
where
    H: Header,
    GlobalConfig<K, Versioned<REUSE, H, V>>: for<'x, 'y> Config<
            Store: Default,
            Manager<'x> = VManager<'x, K, Versioned<REUSE, H, V>>,
            Arena<'y, 'x> = VArena<'y, 'x, K, Versioned<REUSE, H, V>, H>,
        >,
{
    pub fn new(guard: Guard<'id>, manager_guard: Guard<'man>, header: H) -> Self {
        Self(VArena {
            manager: Arc::new(RwLock::new(UnsafeCell::new(Manager::new(manager_guard)))),
            port:    Arc::new(RwLock::new((guard.into(), header))),
        })
    }
}
