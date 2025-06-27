use std::marker::PhantomData;

use generativity::{Guard, Id};

use super::*;
use crate::alloc::store::*;

#[derive(Debug)]
pub struct XHandle<'id, T: ?Sized> {
    index:    Index,
    _manager: Id<'id>,
    _marker:  PhantomData<fn() -> T>,
}

// TODO: how to handle Debug?
// #[derive(Debug)]
pub struct XManager<'id, K, C>
where
    GlobalConfig<K, C>: Config,
{
    store:   <GlobalConfig<K, C> as Config>::Store,
    id:      Id<'id>,
    _marker: PhantomData<K>,
}
impl<'id, K, const REUSE: bool, V> Manager<'id, K, Exclusive<REUSE, V>>
where
    GlobalConfig<K, Exclusive<REUSE, V>>:
        for<'x> Config<Store: Default, Manager<'x> = XManager<'x, K, Exclusive<REUSE, V>>>,
{
    pub fn new(guard: Guard<'id>) -> Self {
        Self(XManager {
            store:   <GlobalConfig<K, Exclusive<REUSE, V>> as Config>::Store::default(),
            id:      guard.into(),
            _marker: PhantomData,
        })
    }
}
impl<K, const REUSE: bool, V> Manager<'_, K, Exclusive<REUSE, V>>
where
    GlobalConfig<K, Exclusive<REUSE, V>>:
        for<'x> Config<Store: Resizable, Manager<'x> = XManager<'x, K, Exclusive<REUSE, V>>>,
{
    pub fn reserve(&mut self, additional: Length) -> MResult<()> {
        let new_capacity = self.0.store.capacity().checked_add(additional).ok_or_else(|| {
            StoreError::OutofMemory(self.0.store.capacity(), self.0.store.capacity() + additional)
        })?;
        Ok(self.0.store.widen(new_capacity)?)
    }
    /// This will not drop existing items and might cause a memory leak
    /// # Safety
    /// This does not invalidate existing [`XHandle`]s.
    /// Using such a handle is undefined behaviour.
    pub unsafe fn force_clear(&mut self) {
        self.0.store.clear();
    }
    /// This will not drop existing items and might cause a memory leak
    pub fn into_empty(mut self, guard: Guard<'_>) -> Manager<'_, K, Exclusive<REUSE, V>> {
        self.0.store.clear();
        Manager(XManager { store: self.0.store, id: guard.into(), _marker: PhantomData })
    }
}
impl<'id, T, const REUSE: bool, V> Manager<'id, Typed<T>, Exclusive<REUSE, V>>
where
    GlobalConfig<Typed<T>, Exclusive<REUSE, V>>:
        for<'x> Config<Store: Store<T>, Manager<'x> = XManager<'x, Typed<T>, Exclusive<REUSE, V>>>,
{
    pub fn get(&self, handle: &XHandle<'id, T>) -> MResult<&T> {
        Ok(self.0.store.get(handle.index)?)
    }
    pub fn get_mut(&mut self, handle: &mut XHandle<'id, T>) -> MResult<&mut T> {
        Ok(self.0.store.get_mut(handle.index)?)
    }
    pub fn insert_within_capacity(&mut self, data: T) -> Result<XHandle<'id, T>, T> {
        self.0.store.insert_within_capacity(data).map(|index| XHandle {
            index,
            _manager: self.0.id,
            _marker: PhantomData,
        })
    }
}
impl<'id, T, const REUSE: bool, V> Manager<'id, Typed<T>, Exclusive<REUSE, V>>
where
    GlobalConfig<Typed<T>, Exclusive<REUSE, V>>: for<'x> Config<
            Store: Store<T> + GetDisjointMut<Single<T>>,
            Manager<'x> = XManager<'x, Typed<T>, Exclusive<REUSE, V>>,
        >,
{
    pub fn get_disjoint_mut<const N: usize>(
        &mut self,
        handles: [&mut XHandle<'id, T>; N],
    ) -> [&mut T; N] {
        // SAFETY: exclusive handles are disjoint by definition
        unsafe { self.0.store.get_disjoint_unchecked_mut(handles.map(|handle| handle.index)) }
    }
}
impl<'id, T, V> Manager<'id, Typed<T>, Exclusive<true, V>>
where
    GlobalConfig<Typed<T>, Exclusive<true, V>>: for<'x> Config<
            Store: ReusableStore<T>,
            Manager<'x> = XManager<'x, Typed<T>, Exclusive<true, V>>,
        >,
{
    pub fn remove(
        &mut self,
        handle: XHandle<'id, T>,
    ) -> Result<T, (XHandle<'id, T>, ManagerError)> {
        self.0.store.remove(handle.index).map_err(|err| (handle, err.into()))
    }
}
impl<'id, U, const REUSE: bool, V> Manager<'id, Slices<U>, Exclusive<REUSE, V>>
where
    U: RawBytes,
    GlobalConfig<Slices<U>, Exclusive<REUSE, V>>: for<'x> Config<
            Store: MultiStore<U>,
            Manager<'x> = XManager<'x, Slices<U>, Exclusive<REUSE, V>>,
        >,
{
    pub fn len<T>(&self, handle: &XHandle<'id, [T]>) -> MResult<Length> {
        // SAFETY: handle is always valid
        Ok(unsafe { Slices::<U>::read_header::<()>(&self.0.store, handle.index)?.0.0 })
    }
    pub fn get<T>(&self, handle: &XHandle<'id, [T]>) -> MResult<&[T]> {
        // SAFETY: handle is always valid
        let ((len, _), index) =
            unsafe { Slices::<U>::read_header::<()>(&self.0.store, handle.index)? };
        // SAFETY: results of get_length are always valid
        Ok(unsafe { Slices::get_slice(&self.0.store, index, len)? })
    }
    pub fn get_mut<T>(&mut self, handle: &mut XHandle<'id, [T]>) -> MResult<&mut [T]> {
        // SAFETY: handle is always valid
        let ((len, _), index) =
            unsafe { Slices::<U>::read_header::<()>(&self.0.store, handle.index)? };
        // SAFETY: results of get_length are always valid
        Ok(unsafe { Slices::get_slice_mut(&mut self.0.store, index, len)? })
    }
    pub fn insert_within_capacity<T: Copy>(&mut self, data: &[T]) -> Option<XHandle<'id, [T]>> {
        let size =
            Slices::<U>::header_size::<()>() + Slices::<U>::size_of::<T>(data.len() as Length);
        let (index, mut lock) = self.0.store.insert_indirect_within_capacity(size)?;
        // SAFETY: insert_many_* always returns a valid target
        unsafe { Slices::write_slice(data, (), lock.as_mut()) };
        Some(XHandle { index: index.start, _manager: self.0.id, _marker: PhantomData })
    }
}
impl<'id, U, const REUSE: bool, V> Manager<'id, Slices<U>, Exclusive<REUSE, V>>
where
    U: RawBytes,
    GlobalConfig<Slices<U>, Exclusive<REUSE, V>>: for<'x> Config<
            Store: MultiStore<U> + GetDisjointMut<Multi<U>>,
            Manager<'x> = XManager<'x, Slices<U>, Exclusive<REUSE, V>>,
        >,
{
    pub fn get_disjoint_mut<const N: usize, T>(
        &mut self,
        handles: [&mut XHandle<'id, [T]>; N],
    ) -> MResult<[&mut [T]; N]> {
        // SAFETY: exclusive handles are always distinct
        Ok(unsafe {
            Slices::get_disjoint_mut(
                &mut self.0.store,
                handles.map(|handle| handle.index),
                |_: &[((Length, ()), Index)]| Ok::<_, ManagerError>(()),
            )?
        })
    }
}
impl<'id, U, V> Manager<'id, Slices<U>, Exclusive<true, V>>
where
    U: RawBytes,
    GlobalConfig<Slices<U>, Exclusive<true, V>>: for<'x> Config<
            Store: ReusableMultiStore<U>,
            Manager<'x> = XManager<'x, Slices<U>, Exclusive<true, V>>,
        >,
{
    #[expect(clippy::type_complexity)]
    pub fn remove<T: Copy>(
        &mut self,
        handle: XHandle<'id, [T]>,
    ) -> Result<RemoveSliceGuard<'_, U, Exclusive<true, V>>, (XHandle<'id, [T]>, ManagerError)>
    {
        // SAFETY: handle is always valid
        match unsafe { Slices::<U>::read_header::<()>(&self.0.store, handle.index) } {
            // SAFETY: result of `read_header` is always valid
            Ok(((len, _), index)) => unsafe {
                Slices::<U>::delete_slice::<
                    T,
                    <GlobalConfig<Slices<U>, Exclusive<true, V>> as Config>::Store,
                >(&mut self.0.store, index, len)
                .map_err(|err| (handle, err.into()))
            },
            Err(err) => Err((handle, err.into())),
        }
    }
}
impl<'id, U, const REUSE: bool, V> Manager<'id, Mixed<U>, Exclusive<REUSE, V>>
where
    U: RawBytes,
    GlobalConfig<Mixed<U>, Exclusive<REUSE, V>>: for<'x> Config<
            Store: MultiStore<U>,
            Manager<'x> = XManager<'x, Mixed<U>, Exclusive<REUSE, V>>,
        >,
{
    pub fn get<T>(&self, handle: &XHandle<'id, T>) -> MResult<&T> {
        // SAFETY: handle is always valid
        Ok(unsafe { Mixed::get_instance(&self.0.store, handle.index)? })
    }
    pub fn get_mut<T>(&mut self, handle: &mut XHandle<'id, T>) -> MResult<&mut T> {
        // SAFETY: handle is always valid
        Ok(unsafe { Mixed::get_instance_mut(&mut self.0.store, handle.index)? })
    }
    pub fn insert_within_capacity<T>(&mut self, data: T) -> Result<XHandle<'id, T>, T> {
        let size = Mixed::<U>::size_of::<T>();
        match self.0.store.insert_indirect_within_capacity(size) {
            Some((index, mut lock)) => {
                // SAFETY: insert_many_* always returns a valid target
                unsafe { Mixed::write_instance(data, lock.as_mut()) };
                Ok(XHandle { index: index.start, _manager: self.0.id, _marker: PhantomData })
            },
            None => Err(data),
        }
    }
}
impl<'id, U, const REUSE: bool, V> Manager<'id, Mixed<U>, Exclusive<REUSE, V>>
where
    U: RawBytes,
    GlobalConfig<Mixed<U>, Exclusive<REUSE, V>>: for<'x> Config<
            Store: MultiStore<U> + GetDisjointMut<Multi<U>>,
            Manager<'x> = XManager<'x, Mixed<U>, Exclusive<REUSE, V>>,
        >,
{
    pub fn get_disjoint_mut<const N: usize, T>(
        &mut self,
        handles: [&mut XHandle<'id, T>; N],
    ) -> MResult<[&mut T; N]> {
        // SAFETY: exclusive handles are always distinct
        Ok(unsafe {
            Mixed::get_disjoint_unchecked_mut(
                &mut self.0.store,
                handles.map(|handle| handle.index),
            )?
        })
    }
}
impl<'id, U, V> Manager<'id, Mixed<U>, Exclusive<true, V>>
where
    U: RawBytes,
    GlobalConfig<Mixed<U>, Exclusive<true, V>>: for<'x> Config<
            Store: ReusableMultiStore<U>,
            Manager<'x> = XManager<'x, Mixed<U>, Exclusive<true, V>>,
        >,
{
    pub fn remove<T>(
        &mut self,
        handle: XHandle<'id, T>,
    ) -> Result<T, (XHandle<'id, T>, ManagerError)> {
        // SAFETY: handle is always valid
        unsafe {
            Mixed::<U>::delete_instance(&mut self.0.store, handle.index)
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
    /// use generativity::make_guard;
    /// use niche_collections::alloc::{
    ///     manager::{Typed, XManager},
    ///     store::FreelistStore,
    /// };
    ///
    /// make_guard!(guard);
    /// let managerB = Manager::<Typed<bool>, Exclusive>::new(guard);
    /// make_guard!(guard);
    /// let mut managerA = Manager::<Typed<bool>, Exclusive>::new(guard);
    /// managerA.reserve(1).unwrap();
    /// let handle = managerA.insert_within_capacity(true).unwrap();
    /// let val = managerB.get(&handle).unwrap();
    /// ```
    #[expect(dead_code)]
    pub struct HandlesAreBranded;

    #[test]
    fn can_use_inner_store() {
        make_guard!(guard);
        let mut manager = Manager::<Typed<i16>, Exclusive<true>>::new(guard);
        assert_eq!(Ok(()), manager.reserve(1));
        let handle = manager
            .insert_within_capacity(42)
            .expect("insert with spare capacity should be successful");
        assert_eq!(Ok(&42), manager.get(&handle));
        assert!(matches!(manager.remove(handle), Ok(42)));
    }
}
