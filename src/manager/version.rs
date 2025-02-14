use crate::*;
use generativity::{Guard, Id};
use manager::ManagerError;
use std::{marker::PhantomData, num::NonZeroU32};

pub type Version = NonZeroU32;
// SAFETY: 1 is not zero
const VERSION1: Version = unsafe { Version::new_unchecked(1) };

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VersionHandle<'man, T> {
    index: Index,
    version: Version,
    manager: Id<'man>,
    _marker: PhantomData<fn() -> T>,
}
pub type VHandle<'man, T> = VersionHandle<'man, T>;

pub struct VersionManager<'id, T, S> {
    store: S,
    version: Version,
    dirty: bool,
    id: Id<'id>,
    _marker: PhantomData<T>,
}
pub type VManager<'id, T, S> = VersionManager<'id, T, S>;
impl<'id, T, S> VManager<'id, T, S>
where
    S: Default,
{
    pub fn new(guard: Guard<'id>) -> Self {
        Self {
            store: S::default(),
            version: VERSION1,
            dirty: false,
            id: guard.into(),
            _marker: PhantomData,
        }
    }
}
impl<'id, T, S: Store<(Version, T)>> VManager<'id, T, S> {
    pub fn get(&self, handle: VHandle<'id, T>) -> Result<&T, ManagerError> {
        self.store.get(handle.index).map_err(ManagerError::from).and_then(|(v, data)| {
            (*v == handle.version)
                .then_some(data)
                .ok_or(ManagerError::BadHandle("version mismatch"))
        })
    }
    pub fn get_mut(&mut self, handle: VHandle<'id, T>) -> Result<&mut T, ManagerError> {
        self.store.get_mut(handle.index).map_err(ManagerError::from).and_then(|(v, data)| {
            (*v == handle.version)
                .then_some(data)
                .ok_or(ManagerError::BadHandle("version mismatch"))
        })
    }
    pub fn insert_within_capacity(&mut self, data: T) -> Result<VHandle<'id, T>, T> {
        if self.dirty {
            self.dirty = false;
            self.version = self.version.checked_add(1).unwrap_or(VERSION1);
        }
        self.store.insert_within_capacity((self.version, data)).map_err(|(_, data)| data).map(
            |index| VHandle {
                index,
                version: self.version,
                manager: self.id,
                _marker: PhantomData,
            },
        )
    }
    pub fn reserve(&mut self, additional: usize) -> Result<(), ManagerError> {
        self.store.reserve(additional).map_err(ManagerError::from)
    }
    pub fn clear(&mut self) {
        self.dirty = true;
        self.store.clear();
    }
}
impl<'id, T, S: ReusableStore<(Version, T)>> VManager<'id, T, S> {
    pub fn remove(&mut self, handle: VHandle<'id, T>) -> Result<T, ManagerError> {
        if self.store.get(handle.index).map_err(ManagerError::from)?.0 != handle.version {
            return Err(ManagerError::BadHandle("version mismatch"));
        }
        let removed = self.store.remove(handle.index).map_err(ManagerError::from)?;
        self.dirty = true;
        Ok(removed.1)
    }
}
// TODO: MultiStore impl
// TODO: testing
