use std::{cell::UnsafeCell, marker::PhantomData, mem::transmute, sync::Arc};

use generativity::{Guard, Id};
use parking_lot::{RwLock, RwLockReadGuard, RwLockUpgradableReadGuard, RwLockWriteGuard};

use super::ArenaError;
use crate::{manager::*, store::*};

type ManagerCell<'man, K, S> = UnsafeCell<VManager<'man, K, S>>;
type ArcLock<T> = Arc<RwLock<T>>;

macro_rules! map_handle {
    ($handle:ident<$t:ty> $from:lifetime -> $to:lifetime) => {
        // SAFETY: there is no safety here
        unsafe { transmute::<VHandle<$from, $t>, VHandle<$to, $t>>($handle) }
    };
}
#[derive(Debug, Clone, Copy)]
pub struct HandleMap<'from, 'to>(PhantomData<fn(&'from ()) -> &'to ()>);
impl<'from, 'to> HandleMap<'from, 'to> {
    pub fn apply<T>(self, handle: VHandle<'from, T>) -> VHandle<'to, T> {
        // SAFETY: the existance of self implies 'to was joint with 'from
        map_handle!(handle<T> 'from -> 'to)
    }
    pub fn chain<'next>(self, _other: HandleMap<'to, 'next>) -> HandleMap<'from, 'next> {
        HandleMap(PhantomData)
    }
}

#[derive(Debug, Clone)]
pub struct VersionArena<'id, 'man, K: Kind, S> {
    manager: ArcLock<ManagerCell<'man, K, S>>,
    port:    ArcLock<Id<'id>>,
}
pub type VArena<'id, 'man, K, S> = VersionArena<'id, 'man, K, S>;
impl<'id, 'man, K: Kind, S> VArena<'id, 'man, K, S> {
    pub fn split<'new>(&self, guard: Guard<'new>) -> VArena<'new, 'man, K, S> {
        VArena { manager: self.manager.clone(), port: Arc::new(RwLock::new(guard.into())) }
    }
    pub fn join<'other>(&self, _other: VArena<'other, 'man, K, S>) -> HandleMap<'other, 'id> {
        HandleMap(PhantomData)
    }
    pub fn read(&self) -> VersionArenaReadGuard<'_, 'id, 'man, K, S> {
        VersionArenaReadGuard { manager: self.manager.read(), port: self.port.read() }
    }
    pub fn write(&mut self) -> VersionArenaWriteGuard<'_, 'id, 'man, K, S> {
        VersionArenaWriteGuard { manager: self.manager.read(), port: self.port.write() }
    }
    pub fn alloc(&mut self) -> VersionArenaAllocGuard<'_, 'id, 'man, K, S> {
        VersionArenaAllocGuard {
            manager: self.manager.upgradable_read(),
            port:    self.port.write(),
        }
    }
}
impl<'id, 'man, K: Kind, S> VArena<'id, 'man, K, S>
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
pub struct VersionArenaReadGuard<'a, 'id, 'man, K: Kind, S> {
    manager: RwLockReadGuard<'a, ManagerCell<'man, K, S>>,
    port:    RwLockReadGuard<'a, Id<'id>>,
}
#[derive(Debug)]
pub struct VersionArenaWriteGuard<'a, 'id, 'man, K: Kind, S> {
    manager: RwLockReadGuard<'a, ManagerCell<'man, K, S>>,
    port:    RwLockWriteGuard<'a, Id<'id>>,
}
#[derive(Debug)]
pub struct VersionArenaAllocGuard<'a, 'id, 'man, K: Kind, S> {
    manager: RwLockUpgradableReadGuard<'a, ManagerCell<'man, K, S>>,
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
macro_rules! impl_read {
    ($type:ident) => {
        impl<'id, 'man, T, S> $type<'_, 'id, 'man, Typed<T>, S>
        where
            S: Store<(Version, T)>,
        {
            pub fn get(&self, handle: VHandle<'id, T>) -> Result<&T, ArenaError> {
                Ok(manager!(ref self).get(map_handle!(handle<T> 'id -> 'man))?)
            }
        }
        impl<'id, 'man, U, S> $type<'_, 'id, 'man, Slices<U>, S>
        where
            U: RawBytes,
            S: MultiStore<U>,
        {
            pub fn get<T>(&self, handle: VHandle<'id, [T]>) -> Result<&[T], ArenaError> {
                Ok(manager!(ref self).get(map_handle!(handle<[T]> 'id -> 'man))?)
            }
        }
        impl<'id, 'man, U, S> $type<'_, 'id, 'man, Mixed<U>, S>
        where
            U: RawBytes,
            S: MultiStore<U>,
        {
            pub fn get<T>(&self, handle: VHandle<'id, T>) -> Result<&T, ArenaError> {
                Ok(manager!(ref self).get(map_handle!(handle<T> 'id -> 'man))?)
            }
        }
    };
}
impl_read!(VersionArenaReadGuard);
impl_read!(VersionArenaWriteGuard);
impl_read!(VersionArenaAllocGuard);
macro_rules! impl_write {
    ($type:ident) => {
        impl<'id, 'man, T, S> $type<'_, 'id, 'man, Typed<T>, S>
        where
            S: Store<(Version, T)>,
        {
            pub fn get_mut(&mut self, handle: VHandle<'id, T>) -> Result<&mut T, ArenaError> {
                Ok(manager!(mut self).get_mut(map_handle!(handle<T> 'id -> 'man))?)
            }
            pub fn get_disjoint_mut<const N: usize>(
                &mut self,
                handles: [VHandle<'id, T>; N],
            ) -> Result<[&mut T; N], ArenaError> {
                Ok(manager!(mut self)
                    .get_disjoint_mut(handles.map(|handle| map_handle!(handle<T> 'id -> 'man)))?)
            }
        }
        impl<'id, 'man, U, S> $type<'_, 'id, 'man, Slices<U>, S>
        where
            U: RawBytes,
            S: MultiStore<U>,
        {
            pub fn get_mut<T>(&mut self, handle: VHandle<'id, [T]>) -> Result<&mut [T], ArenaError> {
                Ok(manager!(mut self).get_mut(map_handle!(handle<[T]> 'id -> 'man))?)
            }
            pub fn get_disjoint_mut<const N: usize, T>(
                &mut self,
                handles: [VHandle<'id, [T]>; N],
            ) -> Result<[&mut [T]; N], ArenaError> {
                Ok(manager!(mut self)
                    .get_disjoint_mut(handles.map(|handle| map_handle!(handle<[T]> 'id -> 'man)))?)
            }
        }
        impl<'id, 'man, U, S> $type<'_, 'id, 'man, Mixed<U>, S>
        where
            U: RawBytes,
            S: MultiStore<U>,
        {
            pub fn get_mut<T>(&mut self, handle: VHandle<'id, T>) -> Result<&mut T, ArenaError> {
                Ok(manager!(mut self).get_mut(map_handle!(handle<T> 'id -> 'man))?)
            }
            pub fn get_disjoint_mut<const N: usize, T>(
                &mut self,
                handles: [VHandle<'id, T>; N],
            ) -> Result<[&mut T; N], ArenaError> {
                Ok(manager!(mut self)
                    .get_disjoint_mut(handles.map(|handle| map_handle!(handle<T> 'id -> 'man)))?)
            }
        }
    };
}
impl_write!(VersionArenaWriteGuard);
impl_write!(VersionArenaAllocGuard);
impl<T, S> VersionArenaAllocGuard<'_, '_, '_, Typed<T>, S>
where
    S: Store<(Version, T)>,
{
    #[rustfmt::skip]
    pub fn reserve(&mut self, additional: usize) -> Result<(), ArenaError> {
        manager!(lock self |manager| Ok(manager.reserve(additional)?))
    }
}
impl<'id, 'man, T, S> VersionArenaAllocGuard<'_, 'id, 'man, Typed<T>, S>
where
    S: Store<(Version, T)>,
{
    pub fn insert_within_capacity(&mut self, data: T) -> Result<VHandle<'id, T>, T> {
        let handle = manager!(mut self).insert_within_capacity(data)?;
        Ok(map_handle!(handle<T> 'man -> 'id))
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
}
impl<'id, 'man, T, S> VersionArenaAllocGuard<'_, 'id, 'man, Typed<T>, S>
where
    S: ReusableStore<(Version, T)>,
{
    pub fn remove(&mut self, handle: VHandle<'id, T>) -> Result<T, ArenaError> {
        Ok(manager!(mut self).remove(map_handle!(handle<T> 'id -> 'man))?)
    }
}
impl<U, S> VersionArenaAllocGuard<'_, '_, '_, Slices<U>, S>
where
    U: RawBytes,
    S: MultiStore<U>,
{
    #[rustfmt::skip]
    pub fn reserve(&mut self, additional: usize) -> Result<(), ArenaError> {
        manager!(lock self |manager| Ok(manager.reserve(additional)?))
    }
}
impl<'id, 'man, U, S> VersionArenaAllocGuard<'_, 'id, 'man, Slices<U>, S>
where
    U: RawBytes,
    S: MultiStore<U>,
{
    pub fn insert_within_capacity<T: Copy>(&mut self, data: &[T]) -> Option<VHandle<'id, [T]>> {
        let handle = manager!(mut self).insert_within_capacity(data)?;
        Some(map_handle!(handle<[T]> 'man -> 'id))
    }
    pub fn insert<T: Copy>(&mut self, data: &[T]) -> Result<VHandle<'id, [T]>, ArenaError> {
        match self.insert_within_capacity(data) {
            Some(handle) => Ok(handle),
            None => {
                self.reserve(
                    Slices::<U>::header_size::<Version>()
                        + Slices::<U>::size_of::<T>(data.len() as Length),
                )?;
                let Some(handle) = self.insert_within_capacity(data) else {
                    unreachable!("insert after reserve should always be successful")
                };
                Ok(handle)
            },
        }
    }
}
impl<'id, 'man, U, S> VersionArenaAllocGuard<'_, 'id, 'man, Slices<U>, S>
where
    U: RawBytes,
    S: ReusableMultiStore<U>,
{
    pub fn remove<T: Copy>(
        &mut self,
        handle: VHandle<'id, [T]>,
    ) -> Result<BeforeRemoveMany<'_, T, impl FnOnce()>, ArenaError> {
        Ok(manager!(mut self).remove(map_handle!(handle<[T]> 'id -> 'man))?)
    }
}
impl<U, S> VersionArenaAllocGuard<'_, '_, '_, Mixed<U>, S>
where
    U: RawBytes,
    S: MultiStore<U>,
{
    #[rustfmt::skip]
    pub fn reserve(&mut self, additional: usize) -> Result<(), ArenaError> {
        manager!(lock self |manager| Ok(manager.reserve(additional)?))
    }
}
impl<'id, 'man, U, S> VersionArenaAllocGuard<'_, 'id, 'man, Mixed<U>, S>
where
    U: RawBytes,
    S: MultiStore<U>,
{
    pub fn insert_within_capacity<T>(&mut self, data: T) -> Result<VHandle<'id, T>, T> {
        let handle = manager!(mut self).insert_within_capacity(data)?;
        Ok(map_handle!(handle<T> 'man -> 'id))
    }
    pub fn insert<T>(&mut self, data: T) -> Result<VHandle<'id, T>, (T, ArenaError)> {
        match self.insert_within_capacity(data) {
            Ok(handle) => Ok(handle),
            Err(data) => {
                if let Err(err) = self.reserve(Mixed::<U>::size_of::<(Version, T)>()) {
                    return Err((data, err));
                }
                let Ok(handle) = self.insert_within_capacity(data) else {
                    unreachable!("insert after reserve should always be successful")
                };
                Ok(handle)
            },
        }
    }
}
impl<'id, 'man, U, S> VersionArenaAllocGuard<'_, 'id, 'man, Mixed<U>, S>
where
    U: RawBytes,
    S: ReusableMultiStore<U>,
{
    pub fn remove<T>(&mut self, handle: VHandle<'id, T>) -> Result<T, ArenaError> {
        Ok(manager!(mut self).remove(map_handle!(handle<T> 'id -> 'man))?)
    }
}
