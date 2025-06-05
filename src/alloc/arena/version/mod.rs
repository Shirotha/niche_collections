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

type ManagerCell<'man, K, S> = UnsafeCell<VManager<'man, K, S>>;
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
pub struct VArena<'id, 'man, K: Kind, S, H: Header = Headless> {
    manager: ArcLock<ManagerCell<'man, K, S>>,
    port:    ArcLock<(Id<'id>, H)>,
}
impl<'id, 'man, K, S, H> VArena<'id, 'man, K, S, H>
where
    K: Kind,
    H: Header,
{
    pub fn split<'new>(&self, guard: Guard<'new>) -> VArena<'new, 'man, K, S, H> {
        VArena {
            manager: self.manager.clone(),
            port:    Arc::new(RwLock::new((guard.into(), H::default()))),
        }
    }
    /// # Safety
    /// After this call accessing `VHandle<'other, _>` causes undefined behaviour.
    /// This can deadlock if `self` is the same as `other`.
    /// Making this safe requires `H` to carry additional data and thus is not being done by default.
    pub unsafe fn join<'other>(
        &self,
        other: VArena<'other, 'man, K, S, H>,
    ) -> HandleMap<'other, 'id> {
        let mut this_port = self.port.write();
        let mut other_port = other.port.write();
        this_port.1.merge(&mut other_port.1);
        HandleMap { _from: other_port.0, _to: this_port.0 }
    }
    pub fn read(&self) -> VArenaReadGuard<'_, 'id, 'man, K, S, H> {
        VArenaReadGuard { manager: self.manager.read(), port: self.port.read() }
    }
    pub fn write(&mut self) -> VArenaWriteGuard<'_, 'id, 'man, K, S, H> {
        VArenaWriteGuard { manager: self.manager.read(), port: self.port.write() }
    }
    pub fn alloc(&mut self) -> VArenaAllocGuard<'_, 'id, 'man, K, S, H> {
        VArenaAllocGuard { manager: self.manager.upgradable_read(), port: self.port.write() }
    }
}
impl<'id, 'man, K: Kind, S, H> VArena<'id, 'man, K, S, H>
where
    S: Default,
    H: Header,
{
    pub fn new(guard: Guard<'id>, manager_guard: Guard<'man>, header: H) -> Self {
        Self {
            manager: Arc::new(RwLock::new(UnsafeCell::new(VManager::new(manager_guard)))),
            port:    Arc::new(RwLock::new((guard.into(), header))),
        }
    }
}
