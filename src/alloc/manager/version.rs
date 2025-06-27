use std::{array, marker::PhantomData, mem::transmute};

use generativity::{Guard, Id};

use super::*;
use crate::alloc::store::*;

const VERSION1: Version = Version::new(1).unwrap();

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct VHandle<'man, T: ?Sized> {
    index:   Index,
    version: Version,
    manager: Id<'man>,
    _marker: PhantomData<fn() -> T>,
}
impl<T: ?Sized> Clone for VHandle<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: ?Sized> Copy for VHandle<'_, T> {}

pub struct VManager<'id, K, C>
where
    GlobalConfig<K, C>: Config,
{
    store:   <GlobalConfig<K, C> as Config>::Store,
    version: Version,
    dirty:   bool,
    id:      Id<'id>,
    _marker: PhantomData<K>,
}
impl<'id, K, const REUSE: bool, H, V> Manager<'id, K, Versioned<REUSE, H, V>>
where
    GlobalConfig<K, Versioned<REUSE, H, V>>:
        for<'x> Config<Store: Default, Manager<'x> = VManager<'x, K, Versioned<REUSE, H, V>>>,
{
    pub fn new(guard: Guard<'id>) -> Self {
        Self(VManager {
            store:   <GlobalConfig<K, Versioned<REUSE, H, V>> as Config>::Store::default(),
            version: VERSION1,
            dirty:   false,
            id:      guard.into(),
            _marker: PhantomData,
        })
    }
}
impl<K, const REUSE: bool, H, V> Manager<'_, K, Versioned<REUSE, H, V>>
where
    GlobalConfig<K, Versioned<REUSE, H, V>>:
        for<'x> Config<Store: Resizable, Manager<'x> = VManager<'x, K, Versioned<REUSE, H, V>>>,
{
    pub fn reserve(&mut self, additional: Length) -> MResult<()> {
        let new_capacity = self.0.store.capacity().checked_add(additional).ok_or_else(|| {
            StoreError::OutofMemory(self.0.store.capacity(), self.0.store.capacity() + additional)
        })?;
        Ok(self.0.store.widen(new_capacity)?)
    }
    /// This will not drop existing items and might cause a memory leak
    pub fn clear(&mut self) {
        self.0.dirty = true;
        self.0.store.clear();
    }
}
impl<'id, T, const REUSE: bool, H, V> Manager<'id, Typed<T>, Versioned<REUSE, H, V>>
where
    GlobalConfig<Typed<T>, Versioned<REUSE, H, V>>: for<'x> Config<
            Store: Store<(Version, T)>,
            Manager<'x> = VManager<'x, Typed<T>, Versioned<REUSE, H, V>>,
        >,
{
    pub fn get(&self, handle: VHandle<'id, T>) -> MResult<&T> {
        let (v, data) = self.0.store.get(handle.index)?;
        (*v == handle.version).then_some(data).ok_or(ManagerError::BadHandle("version mismatch"))
    }
    pub fn get_mut(&mut self, handle: VHandle<'id, T>) -> MResult<&mut T> {
        let (v, data) = self.0.store.get_mut(handle.index)?;
        (*v == handle.version).then_some(data).ok_or(ManagerError::BadHandle("version mismatch"))
    }
    pub fn insert_within_capacity(&mut self, data: T) -> Result<VHandle<'id, T>, T> {
        if self.0.dirty {
            self.0.dirty = false;
            self.0.version = self.0.version.checked_add(1).unwrap_or(VERSION1);
        }
        self.0.store.insert_within_capacity((self.0.version, data)).map_err(|(_, data)| data).map(
            |index| VHandle {
                index,
                version: self.0.version,
                manager: self.0.id,
                _marker: PhantomData,
            },
        )
    }
    pub(crate) fn bump_version(&mut self, mut handle: VHandle<'id, T>) -> MResult<VHandle<'id, T>> {
        let (v, _) = self.0.store.get_mut(handle.index)?;
        if *v != handle.version {
            return Err(ManagerError::BadHandle("version mismatch"));
        }
        handle.version = handle.version.checked_add(1).unwrap_or(VERSION1);
        *v = handle.version;
        Ok(handle)
    }
}
impl<'id, T, const REUSE: bool, H, V> Manager<'id, Typed<T>, Versioned<REUSE, H, V>>
where
    GlobalConfig<Typed<T>, Versioned<REUSE, H, V>>: for<'x> Config<
            Store: Store<(Version, T)> + GetDisjointMut<Single<(Version, T)>>,
            Manager<'x> = VManager<'x, Typed<T>, Versioned<REUSE, H, V>>,
        >,
{
    pub fn get_disjoint_mut<const N: usize>(
        &mut self,
        handles: [VHandle<'id, T>; N],
    ) -> MResult<[&mut T; N]> {
        let entries = self.0.store.get_disjoint_mut(array::from_fn(|i| handles[i].index))?;
        if entries.iter().zip(handles).any(|((v, _), handle)| *v != handle.version) {
            return Err(ManagerError::BadHandle("version mismatch"));
        }
        Ok(entries.map(|(_, data)| data))
    }
}
impl<'id, T, H, V> Manager<'id, Typed<T>, Versioned<true, H, V>>
where
    GlobalConfig<Typed<T>, Versioned<true, H, V>>: for<'x> Config<
            Store: ReusableStore<(Version, T)>,
            Manager<'x> = VManager<'x, Typed<T>, Versioned<true, H, V>>,
        >,
{
    pub fn remove(&mut self, handle: VHandle<'id, T>) -> MResult<T> {
        if self.0.store.get(handle.index)?.0 != handle.version {
            return Err(ManagerError::BadHandle("version mismatch"));
        }
        let removed = self.0.store.remove(handle.index)?;
        self.0.dirty = true;
        Ok(removed.1)
    }
}
impl<'id, U, const REUSE: bool, H, V> Manager<'id, Slices<U>, Versioned<REUSE, H, V>>
where
    U: RawBytes,
    GlobalConfig<Slices<U>, Versioned<REUSE, H, V>>: for<'x> Config<
            Store: MultiStore<U>,
            Manager<'x> = VManager<'x, Slices<U>, Versioned<REUSE, H, V>>,
        >,
{
    pub fn len<T>(&self, handle: VHandle<'id, [T]>) -> MResult<Length> {
        let ((len, v), _) =
            unsafe { Slices::<U>::read_header::<Version>(&self.0.store, handle.index)? };
        (v == handle.version).then_some(len).ok_or(ManagerError::BadHandle("version mismatch"))
    }
    pub fn get<T>(&self, handle: VHandle<'id, [T]>) -> MResult<&[T]> {
        let ((len, v), index) =
            unsafe { Slices::<U>::read_header::<Version>(&self.0.store, handle.index)? };
        if v != handle.version {
            return Err(ManagerError::BadHandle("version mismatch"));
        }
        Ok(unsafe { Slices::<U>::get_slice(&self.0.store, index, len)? })
    }
    pub fn get_mut<T>(&mut self, handle: VHandle<'id, [T]>) -> MResult<&mut [T]> {
        let ((len, v), index) =
            unsafe { Slices::<U>::read_header::<Version>(&self.0.store, handle.index)? };
        if v != handle.version {
            return Err(ManagerError::BadHandle("version mismatch"));
        }
        Ok(unsafe { Slices::<U>::get_slice_mut(&mut self.0.store, index, len)? })
    }
    pub fn insert_within_capacity<T: Copy>(&mut self, data: &[T]) -> Option<VHandle<'id, [T]>> {
        if self.0.dirty {
            self.0.dirty = false;
            self.0.version = self.0.version.checked_add(1).unwrap_or(VERSION1);
        }
        let size =
            Slices::<U>::header_size::<Version>() + Slices::<U>::size_of::<T>(data.len() as u32);
        let (index, mut lock) = self.0.store.insert_indirect_within_capacity(size)?;
        unsafe { Slices::<U>::write_slice(data, self.0.version, lock.as_mut()) };
        Some(VHandle {
            index:   index.start,
            version: self.0.version,
            manager: self.0.id,
            _marker: PhantomData,
        })
    }
    pub(crate) fn bump_version<T>(
        &mut self,
        mut handle: VHandle<'id, [T]>,
    ) -> MResult<VHandle<'id, [T]>> {
        let ((len, v), _) =
            unsafe { Slices::<U>::read_header::<Version>(&self.0.store, handle.index)? };
        if v != handle.version {
            return Err(ManagerError::BadHandle("version mismatch"));
        }
        handle.version = handle.version.checked_add(1).unwrap_or(VERSION1);
        let dst = self.0.store.get_mut(Slices::<U>::header_range::<Version>(handle.index)?)?;
        // SAFETY: transmuting to MaybeUninit is always valid
        let dst = unsafe { transmute::<&mut [U], &mut [MaybeUninit<U>]>(dst) };
        Slices::<U>::write_header(len, handle.version, dst);
        Ok(handle)
    }
}
impl<'id, U, const REUSE: bool, H, V> Manager<'id, Slices<U>, Versioned<REUSE, H, V>>
where
    U: RawBytes,
    GlobalConfig<Slices<U>, Versioned<REUSE, H, V>>: for<'x> Config<
            Store: MultiStore<U> + GetDisjointMut<Multi<U>>,
            Manager<'x> = VManager<'x, Slices<U>, Versioned<REUSE, H, V>>,
        >,
{
    pub fn get_disjoint_mut<const N: usize, T>(
        &mut self,
        handles: [VHandle<'id, [T]>; N],
    ) -> MResult<[&mut [T]; N]> {
        Ok(unsafe {
            Slices::<U>::get_disjoint_mut(
                &mut self.0.store,
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
}
impl<'id, U, H, V> Manager<'id, Slices<U>, Versioned<true, H, V>>
where
    U: RawBytes,
    GlobalConfig<Slices<U>, Versioned<true, H, V>>: for<'x> Config<
            Store: ReusableMultiStore<U>,
            Manager<'x> = VManager<'x, Slices<U>, Versioned<true, H, V>>,
        >,
{
    pub fn remove<T: Copy>(
        &mut self,
        handle: VHandle<'id, [T]>,
    ) -> MResult<RemoveSliceGuard<'_, U, Versioned<true, H, V>>> {
        let ((len, v), index) =
            unsafe { Slices::<U>::read_header::<Version>(&self.0.store, handle.index)? };
        if v != handle.version {
            return Err(ManagerError::BadHandle("version mismatch"));
        }
        let lock = unsafe {
            Slices::<U>::delete_slice::<
                T,
                <GlobalConfig<Slices<U>, Versioned<true, H, V>> as Config>::Store,
            >(&mut self.0.store, index, len)?
        };
        self.0.dirty = true;
        Ok(lock)
    }
}
impl<'id, U, const REUSE: bool, H, V> Manager<'id, Mixed<U>, Versioned<REUSE, H, V>>
where
    U: RawBytes,
    GlobalConfig<Mixed<U>, Versioned<REUSE, H, V>>: for<'x> Config<
            Store: MultiStore<U>,
            Manager<'x> = VManager<'x, Mixed<U>, Versioned<REUSE, H, V>>,
        >,
{
    pub fn get<T>(&self, handle: VHandle<'id, T>) -> MResult<&T> {
        // SAFETY: handle is always valid
        let (v, data) =
            unsafe { Mixed::<U>::get_instance::<(Version, T)>(&self.0.store, handle.index)? };
        (*v == handle.version).then_some(data).ok_or(ManagerError::BadHandle("version mismatch"))
    }
    pub fn get_mut<T>(&mut self, handle: VHandle<'id, T>) -> MResult<&mut T> {
        // SAFETY: handle is always valid
        let (v, data) = unsafe {
            Mixed::<U>::get_instance_mut::<(Version, T)>(&mut self.0.store, handle.index)?
        };
        (*v == handle.version).then_some(data).ok_or(ManagerError::BadHandle("version mismatch"))
    }
    pub fn insert_within_capacity<T>(&mut self, data: T) -> Result<VHandle<'id, T>, T> {
        if self.0.dirty {
            self.0.dirty = false;
            self.0.version = self.0.version.checked_add(1).unwrap_or(VERSION1);
        }
        let size = Mixed::<U>::size_of::<(Version, T)>();
        match self.0.store.insert_indirect_within_capacity(size) {
            Some((index, mut lock)) => {
                unsafe { Mixed::<U>::write_instance(data, lock.as_mut()) };
                Ok(VHandle {
                    index:   index.start,
                    version: self.0.version,
                    manager: self.0.id,
                    _marker: PhantomData,
                })
            },
            None => Err(data),
        }
    }
    pub(crate) fn bump_version<T>(
        &mut self,
        mut handle: VHandle<'id, T>,
    ) -> MResult<VHandle<'id, T>> {
        let (v, _) = unsafe {
            Mixed::<U>::get_instance_mut::<(Version, T)>(&mut self.0.store, handle.index)?
        };
        if *v != handle.version {
            return Err(ManagerError::BadHandle("version mismatch"));
        }
        handle.version = handle.version.checked_add(1).unwrap_or(VERSION1);
        *v = handle.version;
        Ok(handle)
    }
}
impl<'id, U, const REUSE: bool, H, V> Manager<'id, Mixed<U>, Versioned<REUSE, H, V>>
where
    U: RawBytes,
    GlobalConfig<Mixed<U>, Versioned<REUSE, H, V>>: for<'x> Config<
            Store: MultiStore<U> + GetDisjointMut<Multi<U>>,
            Manager<'x> = VManager<'x, Mixed<U>, Versioned<REUSE, H, V>>,
        >,
{
    pub fn get_disjoint_mut<const N: usize, T>(
        &mut self,
        handles: [VHandle<'id, T>; N],
    ) -> MResult<[&mut T; N]> {
        // SAFETY: handles are always valid
        let result = unsafe {
            Mixed::<U>::get_disjoint_mut::<N, (Version, T)>(
                &mut self.0.store,
                handles.each_ref().map(|handle| handle.index),
            )?
        };
        map_result(handles.iter().zip(result), |(handle, (v, data))| {
            (*v == handle.version)
                .then_some(data)
                .ok_or(ManagerError::BadHandle("version mismatch"))
        })
    }
}
impl<'id, U, H, V> Manager<'id, Mixed<U>, Versioned<true, H, V>>
where
    U: RawBytes,
    GlobalConfig<Mixed<U>, Versioned<true, H, V>>: for<'x> Config<
            Store: ReusableMultiStore<U>,
            Manager<'x> = VManager<'x, Mixed<U>, Versioned<true, H, V>>,
        >,
{
    pub fn remove<T>(&mut self, handle: VHandle<'id, T>) -> MResult<T> {
        let (v, _) =
            unsafe { Mixed::<U>::get_instance::<(Version, T)>(&self.0.store, handle.index)? };
        if *v != handle.version {
            return Err(ManagerError::BadHandle("version mismatch"));
        }
        let (_, data) = unsafe {
            Mixed::<U>::delete_instance::<(Version, T)>(&mut self.0.store, handle.index)?
        };
        self.0.dirty = true;
        Ok(data)
    }
}
// TODO: testing
