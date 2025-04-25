use std::{cell::UnsafeCell, mem::transmute, sync::Arc};

use generativity::{Guard, Id};
use parking_lot::{RwLock, RwLockReadGuard, RwLockUpgradableReadGuard, RwLockWriteGuard};

use super::ArenaError;
use crate::{manager::*, store::*};

type ManagerCell<'man, T, S> = UnsafeCell<VManager<'man, T, S>>;
type ArcLock<T> = Arc<RwLock<T>>;

#[derive(Debug, Clone)]
pub struct VersionArena<'id, 'man, T, S> {
    manager: ArcLock<ManagerCell<'man, T, S>>,
    port:    ArcLock<Id<'id>>,
}
pub type VArena<'id, 'man, T, S> = VersionArena<'id, 'man, T, S>;
impl<'id, 'man, T, S> VArena<'id, 'man, T, S> {
    pub fn split<'new>(&self, guard: Guard<'new>) -> VArena<'new, 'man, T, S> {
        VArena { manager: self.manager.clone(), port: Arc::new(RwLock::new(guard.into())) }
    }
    pub fn read(&self) -> VersionArenaReadGuard<'_, 'id, 'man, T, S> {
        VersionArenaReadGuard { manager: self.manager.read(), port: self.port.read() }
    }
    pub fn write(&mut self) -> VersionArenaWriteGuard<'_, 'id, 'man, T, S> {
        VersionArenaWriteGuard { manager: self.manager.read(), port: self.port.write() }
    }
    pub fn alloc(&mut self) -> VersionArenaAllocGuard<'_, 'id, 'man, T, S> {
        VersionArenaAllocGuard {
            manager: self.manager.upgradable_read(),
            port:    self.port.write(),
        }
    }
}
impl<'id, 'man, T, S> VArena<'id, 'man, T, S>
where
    S: Default,
{
    pub fn new(guard: Guard<'id>, manager_guard: Guard<'man>) -> Self {
        Self {
            manager: Arc::new(RwLock::new(UnsafeCell::new(VManager::new(manager_guard)))),
            port:    Arc::new(RwLock::new(guard.into())),
        }
    }
}
#[derive(Debug)]
pub struct VersionArenaReadGuard<'a, 'id, 'man, T, S> {
    manager: RwLockReadGuard<'a, ManagerCell<'man, T, S>>,
    port:    RwLockReadGuard<'a, Id<'id>>,
}
#[derive(Debug)]
pub struct VersionArenaWriteGuard<'a, 'id, 'man, T, S> {
    manager: RwLockReadGuard<'a, ManagerCell<'man, T, S>>,
    port:    RwLockWriteGuard<'a, Id<'id>>,
}
#[derive(Debug)]
pub struct VersionArenaAllocGuard<'a, 'id, 'man, T, S> {
    manager: RwLockUpgradableReadGuard<'a, ManagerCell<'man, T, S>>,
    port:    RwLockWriteGuard<'a, Id<'id>>,
}
macro_rules! manager {
    (ref $this:ident) => {
        // SAFETY: manager always holds a valid value
        unsafe { $this.manager.get().as_ref().unwrap_unchecked() }
    };
    (mut $this:ident) => {
        // SAFETY: manager always holds a valid value
        unsafe { $this.manager.get().as_mut().unwrap_unchecked() }
    };
    (lock $this:ident |$manager:ident| $body:expr) => {
        $this.manager.with_upgraded(|manager| {
            let $manager = unsafe { manager.get().as_mut().unwrap_unchecked() };
            $body
        })
    };
}
macro_rules! rehandle {
    ($handle:ident<$t:path> $from:lifetime -> $to:lifetime) => {
        unsafe { transmute::<VHandle<$from, T>, VHandle<$to, T>>($handle) }
    };
}
macro_rules! impl_read {
    ($type:ident) => {
        impl<'id, 'man, T, S> $type<'_, 'id, 'man, T, S>
        where
            S: Store<(Version, T)>,
        {
            pub fn get(&self, handle: VHandle<'id, T>) -> Result<&T, ArenaError> {
                manager!(ref self).get(rehandle!(handle<T> 'id -> 'man)).map_err(ArenaError::from)
            }
        }

    };
}
impl_read!(VersionArenaReadGuard);
impl_read!(VersionArenaWriteGuard);
impl_read!(VersionArenaAllocGuard);
macro_rules! impl_write {
    ($type:ident) => {
        impl<'id, 'man, T, S> $type<'_, 'id, 'man, T, S>
        where
            S: Store<(Version, T)>,
        {
            pub fn get_mut(&mut self, handle: VHandle<'id, T>) -> Result<&mut T, ArenaError> {
                manager!(mut self).get_mut(rehandle!(handle<T> 'id -> 'man)).map_err(ArenaError::from)
            }
            pub fn get_disjoint_mut<const N: usize>(
                &mut self,
                handles: [VHandle<'id, T>; N],
            ) -> Result<[&mut T; N], ArenaError> {
                manager!(mut self)
                    .get_disjoint_mut(handles.map(|handle| rehandle!(handle<T> 'id -> 'man)))
                    .map_err(ArenaError::from)
            }
        }

    };
}
impl_write!(VersionArenaWriteGuard);
impl_write!(VersionArenaAllocGuard);
impl<'id, 'man, T, S> VersionArenaAllocGuard<'_, 'id, 'man, T, S>
where
    S: Store<(Version, T)>,
{
    pub fn insert_within_capacity(&mut self, data: T) -> Result<VHandle<'id, T>, T> {
        let handle = manager!(mut self).insert_within_capacity(data)?;
        Ok(rehandle!(handle<T> 'man -> 'id))
    }
    #[rustfmt::skip]
    pub fn reserve(&mut self, additional: usize) -> Result<(), ArenaError> {
        manager!(lock self |manager| manager.reserve(additional).map_err(ArenaError::from))
    }
    pub fn insert(&mut self, data: T) -> Result<VHandle<'id, T>, (T, ArenaError)> {
        match self.insert_within_capacity(data) {
            Ok(handle) => Ok(handle),
            Err(data) => {
                if let Err(err) = self.reserve(1) {
                    return Err((data, err));
                }
                let Ok(handle) = self.insert_within_capacity(data) else {
                    unreachable!("insert after reserve should always be successful")
                };
                Ok(handle)
            },
        }
    }
    // TODO: impl clear somehow
}
impl<'id, 'man, T, S> VersionArenaAllocGuard<'_, 'id, 'man, T, S>
where
    S: ReusableStore<(Version, T)>,
{
    pub fn remove(&mut self, handle: VHandle<'id, T>) -> Result<T, ArenaError> {
        manager!(mut self).remove(rehandle!(handle<T> 'id -> 'man)).map_err(ArenaError::from)
    }
}
