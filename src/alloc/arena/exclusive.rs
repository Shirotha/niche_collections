use std::cell::UnsafeCell;

use generativity::Guard;
use parking_lot::Mutex;

use super::*;
use crate::alloc::{manager::*, store::*};

macro_rules! manager {
    (ref $this:expr) => {
        // SAFETY: manager always holds a valid value
        unsafe { $this.manager.get().as_ref().unwrap_unchecked() }
    };
    (mut $this:expr) => {
        // SAFETY: manager always holds a valid value
        unsafe { $this.manager.get().as_mut().unwrap_unchecked() }
    };
}

#[derive(Debug)]
pub struct XArena<'id, K: Kind, S> {
    manager:    UnsafeCell<XManager<'id, K, S>>,
    alloc_lock: Mutex<()>,
}
// SAFETY: XArena is inherently concurrent by design
unsafe impl<K: Kind, S> Sync for XArena<'_, K, S> {}
impl<'id, K: Kind, S> XArena<'id, K, S>
where
    S: Default,
{
    pub fn new(guard: Guard<'id>) -> Self {
        Self { manager: UnsafeCell::new(XManager::new(guard)), alloc_lock: Mutex::new(()) }
    }
}
impl<K, S> XArena<'_, K, S>
where
    K: Kind,
    S: Resizable,
{
    pub fn reserve(&mut self, additional: Length) -> AResult<()> {
        Ok(self.manager.get_mut().reserve(additional)?)
    }
    /// This will not drop existing items and might cause a memory leak
    /// # Safety
    /// This does not invalidate existing [`XHandle`]s.
    /// Using such a handle is undefined behaviour.
    pub unsafe fn force_clear(&mut self) {
        // SAFETY: assumptions are guarantied by caller
        unsafe { self.manager.get_mut().force_clear() }
    }
    /// This will not drop existing items and might cause a memory leak
    pub fn into_empty(self, guard: Guard<'_>) -> XArena<'_, K, S> {
        XArena {
            manager:    UnsafeCell::new(self.manager.into_inner().into_empty(guard)),
            alloc_lock: self.alloc_lock,
        }
    }
}
impl<'id, T, S> XArena<'id, Typed<T>, S>
where
    S: Store<T>,
{
    pub fn get(&self, handle: &XHandle<'id, T>) -> AResult<&T> {
        Ok(manager!(ref self).get(handle)?)
    }
    pub fn get_mut(&self, handle: &mut XHandle<'id, T>) -> AResult<&mut T> {
        Ok(manager!(mut self).get_mut(handle)?)
    }
    pub fn insert_within_capacity(&self, data: T) -> Result<XHandle<'id, T>, T> {
        let _guard = self.alloc_lock.lock();
        manager!(mut self).insert_within_capacity(data)
    }
    pub fn insert(&mut self, data: T) -> Result<XHandle<'id, T>, (T, ArenaError)> {
        match self.manager.get_mut().insert_within_capacity(data) {
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
impl<'id, T, S> XArena<'id, Typed<T>, S>
where
    S: Store<T> + GetDisjointMut<Single<T>>,
{
    pub fn get_disjoint_mut<const N: usize>(
        &self,
        handles: [&mut XHandle<'id, T>; N],
    ) -> [&mut T; N] {
        manager!(mut self).get_disjoint_mut(handles)
    }
}
impl<'id, T, S> XArena<'id, Typed<T>, S>
where
    S: ReusableStore<T>,
{
    pub fn remove(&self, handle: XHandle<'id, T>) -> Result<T, (XHandle<'id, T>, ArenaError)> {
        let _guard = self.alloc_lock.lock();
        manager!(mut self).remove(handle).map_err(|(handle, err)| (handle, err.into()))
    }
}
impl<'id, U, S> XArena<'id, Slices<U>, S>
where
    U: RawBytes,
    S: MultiStore<U>,
{
    pub fn len<T>(&self, handle: &XHandle<'id, [T]>) -> AResult<Length> {
        Ok(manager!(ref self).len(handle)?)
    }
    pub fn get<T>(&self, handle: &XHandle<'id, [T]>) -> AResult<&[T]> {
        Ok(manager!(ref self).get(handle)?)
    }
    pub fn get_mut<T>(&self, handle: &mut XHandle<'id, [T]>) -> AResult<&mut [T]> {
        Ok(manager!(mut self).get_mut(handle)?)
    }
    pub fn insert_within_capacity<T: Copy>(&self, data: &[T]) -> Option<XHandle<'id, [T]>> {
        let _guard = self.alloc_lock.lock();
        manager!(mut self).insert_within_capacity(data)
    }
    pub fn insert<T: Copy>(&mut self, data: &[T]) -> AResult<XHandle<'id, [T]>> {
        match self.manager.get_mut().insert_within_capacity(data) {
            Some(handle) => Ok(handle),
            None => {
                self.reserve(
                    Slices::<U>::header_size::<()>()
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
impl<'id, U, S> XArena<'id, Slices<U>, S>
where
    U: RawBytes,
    S: MultiStore<U> + GetDisjointMut<Multi<U>>,
{
    pub fn get_disjoint_mut<const N: usize, T>(
        &self,
        handles: [&mut XHandle<'id, [T]>; N],
    ) -> AResult<[&mut [T]; N]> {
        Ok(manager!(mut self).get_disjoint_mut(handles)?)
    }
}
impl<'id, U: RawBytes, S> XArena<'id, Slices<U>, S>
where
    S: ReusableMultiStore<U>,
{
    #[expect(clippy::type_complexity)]
    pub fn remove<T: Copy>(
        &self,
        handle: XHandle<'id, [T]>,
    ) -> Result<<S as RemoveIndirect<Multi<U>>>::Guard<'_>, (XHandle<'id, [T]>, ArenaError)> {
        let _guard = self.alloc_lock.lock();
        manager!(mut self).remove(handle).map_err(|(handle, err)| (handle, err.into()))
    }
}
impl<'id, U, S> XArena<'id, Mixed<U>, S>
where
    U: RawBytes,
    S: MultiStore<U>,
{
    pub fn get<T>(&self, handle: &XHandle<'id, T>) -> AResult<&T> {
        Ok(manager!(ref self).get(handle)?)
    }
    pub fn get_mut<T>(&self, handle: &mut XHandle<'id, T>) -> AResult<&mut T> {
        Ok(manager!(mut self).get_mut(handle)?)
    }
    pub fn insert_within_capacity<T>(&self, data: T) -> Result<XHandle<'id, T>, T> {
        let _guard = self.alloc_lock.lock();
        manager!(mut self).insert_within_capacity(data)
    }
    pub fn insert<T>(&mut self, data: T) -> Result<XHandle<'id, T>, (T, ArenaError)> {
        match self.manager.get_mut().insert_within_capacity(data) {
            Ok(handle) => Ok(handle),
            Err(data) => {
                if let Err(err) = self.reserve(Mixed::<U>::size_of::<T>()) {
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
impl<'id, U, S> XArena<'id, Mixed<U>, S>
where
    U: RawBytes,
    S: MultiStore<U> + GetDisjointMut<Multi<U>>,
{
    pub fn get_disjoint_mut<const N: usize, T>(
        &self,
        handles: [&mut XHandle<'id, T>; N],
    ) -> AResult<[&mut T; N]> {
        Ok(manager!(mut self).get_disjoint_mut(handles)?)
    }
}
impl<'id, U: RawBytes, S> XArena<'id, Mixed<U>, S>
where
    S: ReusableMultiStore<U>,
{
    pub fn remove<T>(&self, handle: XHandle<'id, T>) -> Result<T, (XHandle<'id, T>, ArenaError)> {
        let _guard = self.alloc_lock.lock();
        manager!(mut self).remove(handle).map_err(|(handle, err)| (handle, err.into()))
    }
}
