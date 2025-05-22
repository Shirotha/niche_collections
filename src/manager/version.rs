use std::{array, marker::PhantomData, num::NonZeroU32};

use generativity::{Guard, Id};

use super::*;
use crate::store::*;

pub type Version = NonZeroU32;
const VERSION1: Version = Version::new(1).unwrap();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VersionHandle<'man, T: ?Sized> {
    index:   Index,
    version: Version,
    manager: Id<'man>,
    _marker: PhantomData<fn() -> T>,
}
pub type VHandle<'man, T> = VersionHandle<'man, T>;

pub struct VersionManager<'id, K: Kind, S> {
    store:   S,
    version: Version,
    dirty:   bool,
    id:      Id<'id>,
    _marker: PhantomData<K>,
}
pub type VManager<'id, K, S> = VersionManager<'id, K, S>;
impl<'id, K: Kind, S> VManager<'id, K, S>
where
    S: Default,
{
    pub fn new(guard: Guard<'id>) -> Self {
        Self {
            store:   S::default(),
            version: VERSION1,
            dirty:   false,
            id:      guard.into(),
            _marker: PhantomData,
        }
    }
}
impl<S, T> VManager<'_, Typed<T>, S>
where
    S: Store<(Version, T)>,
{
    pub fn reserve(&mut self, additional: usize) -> Result<(), ManagerError> {
        self.store.reserve(additional).map_err(ManagerError::from)
    }
    /// This will not drop existing items and might cause a memory leak
    pub fn clear(&mut self) {
        self.dirty = true;
        self.store.clear();
    }
}
impl<U, S> VManager<'_, Slices<U>, S>
where
    U: RawBytes,
    S: MultiStore<U>,
{
    pub fn reserve(&mut self, additional: usize) -> Result<(), ManagerError> {
        self.store.reserve(additional).map_err(ManagerError::from)
    }
    /// This will not drop existing items and might cause a memory leak
    pub fn clear(&mut self) {
        self.dirty = true;
        self.store.clear();
    }
}
impl<U, S> VManager<'_, Mixed<U>, S>
where
    U: RawBytes,
    S: MultiStore<U>,
{
    pub fn reserve(&mut self, additional: usize) -> Result<(), ManagerError> {
        self.store.reserve(additional).map_err(ManagerError::from)
    }
    /// This will not drop existing items and might cause a memory leak
    pub fn clear(&mut self) {
        self.dirty = true;
        self.store.clear();
    }
}
impl<'id, T, S> VManager<'id, Typed<T>, S>
where
    S: Store<(Version, T)>,
{
    pub fn get(&self, handle: VHandle<'id, T>) -> Result<&T, ManagerError> {
        let (v, data) = self.store.get(handle.index)?;
        (*v == handle.version).then_some(data).ok_or(ManagerError::BadHandle("version mismatch"))
    }
    pub fn get_mut(&mut self, handle: VHandle<'id, T>) -> Result<&mut T, ManagerError> {
        let (v, data) = self.store.get_mut(handle.index)?;
        (*v == handle.version).then_some(data).ok_or(ManagerError::BadHandle("version mismatch"))
    }
    pub fn get_disjoint_mut<const N: usize>(
        &mut self,
        handles: [VHandle<'id, T>; N],
    ) -> Result<[&mut T; N], ManagerError> {
        let entries = self.store.get_disjoint_mut(array::from_fn(|i| handles[i].index))?;
        if entries.iter().zip(handles).any(|((v, _), handle)| *v != handle.version) {
            return Err(ManagerError::BadHandle("version mismatch"));
        }
        Ok(entries.map(|(_, data)| data))
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
}
impl<'id, T, S> VManager<'id, Typed<T>, S>
where
    S: ReusableStore<(Version, T)>,
{
    pub fn remove(&mut self, handle: VHandle<'id, T>) -> Result<T, ManagerError> {
        if self.store.get(handle.index)?.0 != handle.version {
            return Err(ManagerError::BadHandle("version mismatch"));
        }
        let removed = self.store.remove(handle.index)?;
        self.dirty = true;
        Ok(removed.1)
    }
}
impl<'id, U, S> VManager<'id, Slices<U>, S>
where
    U: RawBytes,
    S: MultiStore<U>,
{
    pub fn len<T>(&self, handle: VHandle<'id, [T]>) -> Result<Length, ManagerError> {
        let ((len, v), _) =
            unsafe { Slices::<U>::read_header::<Version>(&self.store, handle.index)? };
        (v == handle.version).then_some(len).ok_or(ManagerError::BadHandle("version mismatch"))
    }
    pub fn get<T>(&self, handle: VHandle<'id, [T]>) -> Result<&[T], ManagerError> {
        let ((len, v), index) =
            unsafe { Slices::<U>::read_header::<Version>(&self.store, handle.index)? };
        if v != handle.version {
            return Err(ManagerError::BadHandle("version mismatch"));
        }
        Ok(unsafe { Slices::<U>::get_slice(&self.store, index, len)? })
    }
    pub fn get_mut<T>(&mut self, handle: VHandle<'id, [T]>) -> Result<&mut [T], ManagerError> {
        let ((len, v), index) =
            unsafe { Slices::<U>::read_header::<Version>(&self.store, handle.index)? };
        if v != handle.version {
            return Err(ManagerError::BadHandle("version mismatch"));
        }
        Ok(unsafe { Slices::<U>::get_slice_mut(&mut self.store, index, len)? })
    }
    pub fn get_disjoint_mut<const N: usize, T>(
        &mut self,
        handles: [VHandle<'id, [T]>; N],
    ) -> Result<[&mut [T]; N], ManagerError> {
        Ok(unsafe {
            Slices::<U>::get_disjoint_mut(
                &mut self.store,
                handles.each_ref().map(|handle| handle.index),
                |headers: &[((Length, Version), Index)]| {
                    headers
                        .iter()
                        .zip(&handles)
                        .all(|(header, handle)| header.0.1 == handle.version)
                        .then_some(())
                        .ok_or(ManagerError::BadHandle("version mismatch"))
                },
            )?
        })
    }
    pub fn insert_within_capacity<T: Copy>(&mut self, data: &[T]) -> Option<VHandle<'id, [T]>> {
        if self.dirty {
            self.dirty = false;
            self.version = self.version.checked_add(1).unwrap_or(VERSION1);
        }
        let size =
            Slices::<U>::header_size::<Version>() + Slices::<U>::size_of::<T>(data.len() as u32);
        let (index, mut lock) = self.store.insert_many_within_capacity(size)?;
        unsafe { Slices::<U>::write_slice(data, self.version, lock.get_mut()) };
        Some(VHandle { index, version: self.version, manager: self.id, _marker: PhantomData })
    }
}
impl<'id, U, S> VManager<'id, Slices<U>, S>
where
    U: RawBytes,
    S: ReusableMultiStore<U>,
{
    pub fn remove<T: Copy>(
        &mut self,
        handle: VHandle<'id, [T]>,
    ) -> Result<BeforeRemoveMany<'_, T, impl FnOnce()>, ManagerError> {
        let ((len, v), index) =
            unsafe { Slices::<U>::read_header::<Version>(&self.store, handle.index)? };
        if v != handle.version {
            return Err(ManagerError::BadHandle("version mismatch"));
        }
        let lock = unsafe { Slices::<U>::delete_slice(&mut self.store, index, len)? };
        self.dirty = true;
        Ok(lock)
    }
}
impl<'id, U, S> VManager<'id, Mixed<U>, S>
where
    U: RawBytes,
    S: MultiStore<U>,
{
    pub fn get<T>(&self, handle: VHandle<'id, T>) -> Result<&T, ManagerError> {
        // SAFETY: handle is always valid
        let (v, data) =
            unsafe { Mixed::<U>::get_instance::<(Version, T)>(&self.store, handle.index)? };
        (*v == handle.version).then_some(data).ok_or(ManagerError::BadHandle("version mismatch"))
    }
    pub fn get_mut<T>(&mut self, handle: VHandle<'id, T>) -> Result<&mut T, ManagerError> {
        // SAFETY: handle is always valid
        let (v, data) =
            unsafe { Mixed::<U>::get_instance_mut::<(Version, T)>(&mut self.store, handle.index)? };
        (*v == handle.version).then_some(data).ok_or(ManagerError::BadHandle("version mismatch"))
    }
    pub fn get_disjoint_mut<const N: usize, T>(
        &mut self,
        handles: [VHandle<'id, T>; N],
    ) -> Result<[&mut T; N], ManagerError> {
        // SAFETY: handles are always valid
        let result = unsafe {
            Mixed::<U>::get_disjoint_mut::<N, (Version, T)>(
                &mut self.store,
                handles.each_ref().map(|handle| handle.index),
            )?
        };
        map_result(handles.iter().zip(result), |(handle, (v, data))| {
            (*v == handle.version)
                .then_some(data)
                .ok_or(ManagerError::BadHandle("version mismatch"))
        })
    }
    pub fn insert_within_capacity<T>(&mut self, data: T) -> Result<VHandle<'id, T>, T> {
        if self.dirty {
            self.dirty = false;
            self.version = self.version.checked_add(1).unwrap_or(VERSION1);
        }
        let size = Mixed::<U>::size_of::<(Version, T)>();
        match self.store.insert_many_within_capacity(size) {
            Some((index, mut lock)) => {
                unsafe { Mixed::<U>::write_instance(data, lock.get_mut()) };
                Ok(VHandle { index, version: self.version, manager: self.id, _marker: PhantomData })
            },
            None => Err(data),
        }
    }
}
impl<'id, U, S> VManager<'id, Mixed<U>, S>
where
    U: RawBytes,
    S: ReusableMultiStore<U>,
{
    pub fn remove<T>(&mut self, handle: VHandle<'id, T>) -> Result<T, ManagerError> {
        let (v, _) =
            unsafe { Mixed::<U>::get_instance::<(Version, T)>(&self.store, handle.index)? };
        if *v != handle.version {
            return Err(ManagerError::BadHandle("version mismatch"));
        }
        let (_, data) =
            unsafe { Mixed::<U>::delete_instance::<(Version, T)>(&mut self.store, handle.index)? };
        self.dirty = true;
        Ok(data)
    }
}
// TODO: testing
