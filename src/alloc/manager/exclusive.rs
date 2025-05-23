use std::marker::PhantomData;

use generativity::{Guard, Id};

use super::*;
use crate::alloc::store::*;

#[derive(Debug)]
pub struct XHandle<'man, T: ?Sized> {
    index:    Index,
    _manager: Id<'man>,
    _marker:  PhantomData<fn() -> T>,
}

#[derive(Debug)]
pub struct XManager<'id, K: Kind, S> {
    store:   S,
    id:      Id<'id>,
    _marker: PhantomData<K>,
}
impl<'id, K: Kind, S> XManager<'id, K, S>
where
    S: Default,
{
    pub fn new(guard: Guard<'id>) -> Self {
        Self { store: S::default(), id: guard.into(), _marker: PhantomData }
    }
}
impl<K, S> XManager<'_, K, S>
where
    K: Kind,
    S: Store<K::XElement>,
{
    pub fn reserve(&mut self, additional: Length) -> Result<(), ManagerError> {
        Ok(self.store.reserve(additional)?)
    }
    /// This will not drop existing items and might cause a memory leak
    /// # Safety
    /// This does not invalidate existing [`XHandle`]s.
    /// Using such a handle is undefined behaviour.
    pub unsafe fn force_clear(&mut self) {
        self.store.clear();
    }
    /// This will not drop existing items and might cause a memory leak
    pub fn into_empty(mut self, guard: Guard<'_>) -> XManager<'_, K, S> {
        self.store.clear();
        XManager { store: self.store, id: guard.into(), _marker: PhantomData }
    }
}
impl<'id, T, S> XManager<'id, Typed<T>, S>
where
    S: Store<T>,
{
    pub fn get(&self, handle: &XHandle<'id, T>) -> Result<&T, ManagerError> {
        Ok(self.store.get(handle.index)?)
    }
    pub fn get_mut(&mut self, handle: &mut XHandle<'id, T>) -> Result<&mut T, ManagerError> {
        Ok(self.store.get_mut(handle.index)?)
    }
    pub fn get_disjoint_mut<const N: usize>(
        &mut self,
        handles: [&mut XHandle<'id, T>; N],
    ) -> [&mut T; N] {
        // SAFETY: exclusive handles are disjoint by definition
        unsafe { self.store.get_disjoint_unchecked_mut(handles.map(|handle| handle.index)) }
    }
    pub fn insert_within_capacity(&mut self, data: T) -> Result<XHandle<'id, T>, T> {
        self.store.insert_within_capacity(data).map(|index| XHandle {
            index,
            _manager: self.id,
            _marker: PhantomData,
        })
    }
}
impl<'id, T, S> XManager<'id, Typed<T>, S>
where
    S: ReusableStore<T>,
{
    pub fn remove(
        &mut self,
        handle: XHandle<'id, T>,
    ) -> Result<T, (XHandle<'id, T>, ManagerError)> {
        self.store.remove(handle.index).map_err(|err| (handle, err.into()))
    }
}
impl<'id, U, S> XManager<'id, Slices<U>, S>
where
    U: RawBytes,
    S: MultiStore<U>,
{
    pub fn len<T>(&self, handle: &XHandle<'id, [T]>) -> Result<Length, ManagerError> {
        // SAFETY: handle is always valid
        Ok(unsafe { Slices::<U>::read_header::<()>(&self.store, handle.index)?.0.0 })
    }
    pub fn get<T>(&self, handle: &XHandle<'id, [T]>) -> Result<&[T], ManagerError> {
        // SAFETY: handle is always valid
        let ((len, _), index) =
            unsafe { Slices::<U>::read_header::<()>(&self.store, handle.index)? };
        // SAFETY: results of get_length are always valid
        Ok(unsafe { Slices::get_slice(&self.store, index, len)? })
    }
    pub fn get_mut<T>(&mut self, handle: &mut XHandle<'id, [T]>) -> Result<&mut [T], ManagerError> {
        // SAFETY: handle is always valid
        let ((len, _), index) =
            unsafe { Slices::<U>::read_header::<()>(&self.store, handle.index)? };
        // SAFETY: results of get_length are always valid
        Ok(unsafe { Slices::get_slice_mut(&mut self.store, index, len)? })
    }
    pub fn get_disjoint_mut<const N: usize, T>(
        &mut self,
        handles: [&mut XHandle<'id, [T]>; N],
    ) -> Result<[&mut [T]; N], ManagerError> {
        // SAFETY: exclusive handles are always distinct
        Ok(unsafe {
            Slices::get_disjoint_mut(
                &mut self.store,
                handles.map(|handle| handle.index),
                |_: &[((Length, ()), Index)]| Ok::<(), ManagerError>(()),
            )?
        })
    }
    pub fn insert_within_capacity<T: Copy>(&mut self, data: &[T]) -> Option<XHandle<'id, [T]>> {
        let size =
            Slices::<U>::header_size::<()>() + Slices::<U>::size_of::<T>(data.len() as Length);
        let (index, mut lock) = self.store.insert_many_within_capacity(size)?;
        // SAFETY: insert_many_* always returns a valid target
        unsafe { Slices::write_slice(data, (), lock.get_mut()) };
        Some(XHandle { index, _manager: self.id, _marker: PhantomData })
    }
}
impl<'id, U: RawBytes, S> XManager<'id, Slices<U>, S>
where
    S: ReusableMultiStore<U>,
{
    #[expect(clippy::type_complexity)]
    pub fn remove<T: Copy>(
        &mut self,
        handle: XHandle<'id, [T]>,
    ) -> Result<BeforeRemoveMany<'_, T, impl FnOnce()>, (XHandle<'id, [T]>, ManagerError)> {
        // SAFETY: handle is always valid
        match unsafe { Slices::<U>::read_header::<()>(&self.store, handle.index) } {
            // SAFETY: result of `read_header` is always valid
            Ok(((len, _), index)) => unsafe {
                Slices::<U>::delete_slice(&mut self.store, index, len)
                    .map_err(|err| (handle, err.into()))
            },
            Err(err) => Err((handle, err.into())),
        }
    }
}
impl<'id, U, S> XManager<'id, Mixed<U>, S>
where
    U: RawBytes,
    S: MultiStore<U>,
{
    pub fn get<T>(&self, handle: &XHandle<'id, T>) -> Result<&T, ManagerError> {
        // SAFETY: handle is always valid
        Ok(unsafe { Mixed::get_instance(&self.store, handle.index)? })
    }
    pub fn get_mut<T>(&mut self, handle: &mut XHandle<'id, T>) -> Result<&mut T, ManagerError> {
        // SAFETY: handle is always valid
        Ok(unsafe { Mixed::get_instance_mut(&mut self.store, handle.index)? })
    }
    pub fn get_disjoint_mut<const N: usize, T>(
        &mut self,
        handles: [&mut XHandle<'id, T>; N],
    ) -> Result<[&mut T; N], ManagerError> {
        // SAFETY: exclusive handles are always distinct
        Ok(unsafe {
            Mixed::get_disjoint_unchecked_mut(&mut self.store, handles.map(|handle| handle.index))?
        })
    }
    pub fn insert_within_capacity<T>(&mut self, data: T) -> Result<XHandle<'id, T>, T> {
        let size = Mixed::<U>::size_of::<T>();
        match self.store.insert_many_within_capacity(size) {
            Some((index, mut lock)) => {
                // SAFETY: insert_many_* always returns a valid target
                unsafe { Mixed::write_instance(data, lock.get_mut()) };
                Ok(XHandle { index, _manager: self.id, _marker: PhantomData })
            },
            None => Err(data),
        }
    }
}
impl<'id, U: RawBytes, S> XManager<'id, Mixed<U>, S>
where
    S: ReusableMultiStore<U>,
{
    pub fn remove<T>(
        &mut self,
        handle: XHandle<'id, T>,
    ) -> Result<T, (XHandle<'id, T>, ManagerError)> {
        // SAFETY: handle is always valid
        unsafe {
            Mixed::<U>::delete_instance(&mut self.store, handle.index)
                .map_err(|err| (handle, err.into()))
        }
    }
}

#[cfg(any(test, doctest))]
mod test {
    use generativity::make_guard;

    use super::*;

    /// Handles can only be used in the manager that created them.
    /// ```compile_fail
    /// make_guard!(guard);
    /// let managerB = XManager::<Typed<bool>, FreelistStore<bool>>::new(guard);
    /// make_guard!(guard);
    /// let mut managerA = XManager::<Typed<bool>, FreelistStore<bool>>::new(guard);
    /// managerA.reserve(1)?;
    /// let handle = managerA.insert_within_capacity(true).unwrap();
    /// let val = managerB.get(&handle).unwrap();
    /// ```
    #[expect(dead_code)]
    pub struct HandlesAreBranded;

    #[test]
    fn can_use_inner_store() {
        make_guard!(guard);
        let mut manager = XManager::<Typed<i16>, FreelistStore<i16>>::new(guard);
        assert_eq!(Ok(()), manager.reserve(1));
        let handle = manager
            .insert_within_capacity(42)
            .expect("insert with spare capacity should be successful");
        assert_eq!(Ok(&42), manager.get(&handle));
        assert!(matches!(manager.remove(handle), Ok(42)));
    }
}
