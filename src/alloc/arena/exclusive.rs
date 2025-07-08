use std::cell::UnsafeCell;

use generativity::Guard;
use parking_lot::Mutex;

use super::*;
use crate::alloc::{manager::*, store::*};

macro_rules! manager {
    (ref $this:expr) => {
        // SAFETY: manager always holds a valid value
        unsafe { $this.0.manager.get().as_ref().unwrap_unchecked() }
    };
    (mut $this:expr) => {
        // SAFETY: manager always holds a valid value
        unsafe { $this.0.manager.get().as_mut().unwrap_unchecked() }
    };
}

#[derive(Debug)]
pub struct XArena<'id, K, C>
where
    GlobalConfig<K, C>: Config,
{
    manager:    UnsafeCell<Manager<'id, K, C>>,
    alloc_lock: Mutex<()>,
}
// SAFETY: XArena is inherently concurrent by design
unsafe impl<'id, K, const REUSE: bool, V> Sync for Arena<'id, 'id, K, Exclusive<REUSE, V>> where
    GlobalConfig<K, Exclusive<REUSE, V>>: Config
{
}
impl<'id, K, const REUSE: bool, V> Arena<'id, 'id, K, Exclusive<REUSE, V>>
where
    GlobalConfig<K, Exclusive<REUSE, V>>: for<'x> Config<
            Store: Default,
            Manager<'x> = XManager<'x, K, Exclusive<REUSE, V>>,
            Arena<'x, 'x> = XArena<'x, K, Exclusive<REUSE, V>>,
        >,
{
    pub fn new(guard: Guard<'id>) -> Self {
        Self(XArena {
            manager:    UnsafeCell::new(Manager::<K, Exclusive<REUSE, V>>::new(guard)),
            alloc_lock: Mutex::new(()),
        })
    }
}
impl<'id, K, const REUSE: bool, V> Arena<'id, 'id, K, Exclusive<REUSE, V>>
where
    GlobalConfig<K, Exclusive<REUSE, V>>: for<'x> Config<
            Store: Resizable,
            Manager<'x> = XManager<'x, K, Exclusive<REUSE, V>>,
            Arena<'x, 'x> = XArena<'x, K, Exclusive<REUSE, V>>,
        >,
{
    pub fn reserve(&mut self, additional: Length) -> AResult<()> {
        Ok(self.0.manager.get_mut().reserve(additional)?)
    }
    /// This will not drop existing items and might cause a memory leak
    /// # Safety
    /// This does not invalidate existing [`XHandle`]s.
    /// Using such a handle is undefined behaviour.
    pub unsafe fn force_clear(&mut self) {
        // SAFETY: assumptions are guarantied by caller
        unsafe { self.0.manager.get_mut().force_clear() }
    }
    /// This will not drop existing items and might cause a memory leak
    pub fn into_empty(self, guard: Guard<'_>) -> XArena<'_, K, Exclusive<REUSE, V>> {
        XArena {
            manager:    UnsafeCell::new(self.0.manager.into_inner().into_empty(guard)),
            alloc_lock: self.0.alloc_lock,
        }
    }
}
impl<'id, T, const REUSE: bool, V> Arena<'id, 'id, Typed<T>, Exclusive<REUSE, V>>
where
    GlobalConfig<Typed<T>, Exclusive<REUSE, V>>: for<'x> Config<
            Store: Store<T>,
            Manager<'x> = XManager<'x, Typed<T>, Exclusive<REUSE, V>>,
            Arena<'x, 'x> = XArena<'x, Typed<T>, Exclusive<REUSE, V>>,
        >,
{
    pub fn get(&self, handle: &XHandle<'id, T>) -> AResult<&T> {
        Ok(manager!(ref self).get(handle)?)
    }
    #[expect(clippy::mut_from_ref, reason = "mutability is controlled by the handle")]
    pub fn get_mut(&self, handle: &mut XHandle<'id, T>) -> AResult<&mut T> {
        Ok(manager!(mut self).get_mut(handle)?)
    }
    pub fn insert_within_capacity(&self, data: T) -> Result<XHandle<'id, T>, T> {
        let _guard = self.0.alloc_lock.lock();
        manager!(mut self).insert_within_capacity(data)
    }
    pub fn insert(&mut self, data: T) -> Result<XHandle<'id, T>, (T, ArenaError)> {
        match self.0.manager.get_mut().insert_within_capacity(data) {
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
impl<'id, T, const REUSE: bool, V> Arena<'id, 'id, Typed<T>, Exclusive<REUSE, V>>
where
    GlobalConfig<Typed<T>, Exclusive<REUSE, V>>: for<'x> Config<
            Store: Store<T> + GetDisjointMut<Single<T>>,
            Manager<'x> = XManager<'x, Typed<T>, Exclusive<REUSE, V>>,
            Arena<'x, 'x> = XArena<'x, Typed<T>, Exclusive<REUSE, V>>,
        >,
{
    #[expect(clippy::mut_from_ref, reason = "mutability is controlled by the handle")]
    pub fn get_disjoint_mut<const N: usize>(
        &self,
        handles: [&mut XHandle<'id, T>; N],
    ) -> [&mut T; N] {
        manager!(mut self).get_disjoint_mut(handles)
    }
}
impl<'id, T, V> Arena<'id, 'id, Typed<T>, Exclusive<true, V>>
where
    GlobalConfig<Typed<T>, Exclusive<true, V>>: for<'x> Config<
            Store: ReusableStore<T>,
            Manager<'x> = XManager<'x, Typed<T>, Exclusive<true, V>>,
            Arena<'x, 'x> = XArena<'x, Typed<T>, Exclusive<true, V>>,
        >,
{
    pub fn remove(&self, handle: XHandle<'id, T>) -> Result<T, (XHandle<'id, T>, ArenaError)> {
        let _guard = self.0.alloc_lock.lock();
        manager!(mut self).remove(handle).map_err(|(handle, err)| (handle, err.into()))
    }
}
impl<'id, C, const REUSE: bool, V> Arena<'id, 'id, SoA<C>, Exclusive<REUSE, V>>
where
    C: Columns,
    GlobalConfig<SoA<C>, Exclusive<REUSE, V>>: for<'x> Config<
            Store: SoAStore<C>,
            Manager<'x> = XManager<'x, SoA<C>, Exclusive<REUSE, V>>,
            Arena<'x, 'x> = XArena<'x, SoA<C>, Exclusive<REUSE, V>>,
        >,
{
    pub fn query<Q: Query>(&self) -> AResult<XQuery<'id, '_, C, Q>> {
        Ok(manager!(ref self).query()?)
    }
    pub fn insert_within_capacity(&self, data: C) -> Result<XHandle<'id, C>, C> {
        let _guard = self.0.alloc_lock.lock();
        manager!(mut self).insert_within_capacity(data)
    }
    pub fn insert(&mut self, data: C) -> Result<XHandle<'id, C>, (C, ArenaError)> {
        match self.0.manager.get_mut().insert_within_capacity(data) {
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
impl<'id, C, V> Arena<'id, 'id, SoA<C>, Exclusive<true, V>>
where
    C: Columns,
    GlobalConfig<SoA<C>, Exclusive<true, V>>: for<'x> Config<
            Store: ReusableSoAStore<C>,
            Manager<'x> = XManager<'x, SoA<C>, Exclusive<true, V>>,
            Arena<'x, 'x> = XArena<'x, SoA<C>, Exclusive<true, V>>,
        >,
{
    pub fn remove(&self, handle: XHandle<'id, C>) -> Result<C, (XHandle<'id, C>, ArenaError)> {
        let _guard = self.0.alloc_lock.lock();
        manager!(mut self).remove(handle).map_err(|(handle, err)| (handle, err.into()))
    }
}
impl<'id, U, const REUSE: bool, V> Arena<'id, 'id, Slices<U>, Exclusive<REUSE, V>>
where
    U: RawBytes,
    GlobalConfig<Slices<U>, Exclusive<REUSE, V>>: for<'x> Config<
            Store: MultiStore<U>,
            Manager<'x> = XManager<'x, Slices<U>, Exclusive<REUSE, V>>,
            Arena<'x, 'x> = XArena<'x, Slices<U>, Exclusive<REUSE, V>>,
        >,
{
    pub fn len<T>(&self, handle: &XHandle<'id, [T]>) -> AResult<Length> {
        Ok(manager!(ref self).len(handle)?)
    }
    pub fn get<T>(&self, handle: &XHandle<'id, [T]>) -> AResult<&[T]> {
        Ok(manager!(ref self).get(handle)?)
    }
    #[expect(clippy::mut_from_ref, reason = "mutability is controlled by the handle")]
    pub fn get_mut<T>(&self, handle: &mut XHandle<'id, [T]>) -> AResult<&mut [T]> {
        Ok(manager!(mut self).get_mut(handle)?)
    }
    pub fn insert_within_capacity<T: Copy>(&self, data: &[T]) -> Option<XHandle<'id, [T]>> {
        let _guard = self.0.alloc_lock.lock();
        manager!(mut self).insert_within_capacity(data)
    }
    pub fn insert<T: Copy>(&mut self, data: &[T]) -> AResult<XHandle<'id, [T]>> {
        match self.0.manager.get_mut().insert_within_capacity(data) {
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
impl<'id, U, const REUSE: bool, V> Arena<'id, 'id, Slices<U>, Exclusive<REUSE, V>>
where
    U: RawBytes,
    GlobalConfig<Slices<U>, Exclusive<REUSE, V>>: for<'x> Config<
            Store: MultiStore<U> + GetDisjointMut<Multi<U>>,
            Manager<'x> = XManager<'x, Slices<U>, Exclusive<REUSE, V>>,
            Arena<'x, 'x> = XArena<'x, Slices<U>, Exclusive<REUSE, V>>,
        >,
{
    #[expect(clippy::mut_from_ref, reason = "mutability is controlled by the handle")]
    pub fn get_disjoint_mut<const N: usize, T>(
        &self,
        handles: [&mut XHandle<'id, [T]>; N],
    ) -> AResult<[&mut [T]; N]> {
        Ok(manager!(mut self).get_disjoint_mut(handles)?)
    }
}
impl<'id, U, V> Arena<'id, 'id, Slices<U>, Exclusive<true, V>>
where
    U: RawBytes,
    GlobalConfig<Slices<U>, Exclusive<true, V>>: for<'x> Config<
            Store: ReusableMultiStore<U>,
            Manager<'x> = XManager<'x, Slices<U>, Exclusive<true, V>>,
            Arena<'x, 'x> = XArena<'x, Slices<U>, Exclusive<true, V>>,
        >,
{
    #[expect(clippy::type_complexity)]
    pub fn remove<T: Copy>(
        &self,
        handle: XHandle<'id, [T]>,
    ) -> Result<RemoveSliceGuard<'_, U, Exclusive<true, V>>, (XHandle<'id, [T]>, ArenaError)> {
        let _guard = self.0.alloc_lock.lock();
        manager!(mut self).remove(handle).map_err(|(handle, err)| (handle, err.into()))
    }
}
impl<'id, U, const REUSE: bool, V> Arena<'id, 'id, Mixed<U>, Exclusive<REUSE, V>>
where
    U: RawBytes,
    GlobalConfig<Mixed<U>, Exclusive<REUSE, V>>: for<'x> Config<
            Store: MultiStore<U>,
            Manager<'x> = XManager<'x, Mixed<U>, Exclusive<REUSE, V>>,
            Arena<'x, 'x> = XArena<'x, Mixed<U>, Exclusive<REUSE, V>>,
        >,
{
    pub fn get<T>(&self, handle: &XHandle<'id, T>) -> AResult<&T> {
        Ok(manager!(ref self).get(handle)?)
    }
    #[expect(clippy::mut_from_ref, reason = "mutability is controlled by the handle")]
    pub fn get_mut<T>(&self, handle: &mut XHandle<'id, T>) -> AResult<&mut T> {
        Ok(manager!(mut self).get_mut(handle)?)
    }
    pub fn insert_within_capacity<T>(&self, data: T) -> Result<XHandle<'id, T>, T> {
        let _guard = self.0.alloc_lock.lock();
        manager!(mut self).insert_within_capacity(data)
    }
    pub fn insert<T>(&mut self, data: T) -> Result<XHandle<'id, T>, (T, ArenaError)> {
        match self.0.manager.get_mut().insert_within_capacity(data) {
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
impl<'id, U, const REUSE: bool, V> Arena<'id, 'id, Mixed<U>, Exclusive<REUSE, V>>
where
    U: RawBytes,
    GlobalConfig<Mixed<U>, Exclusive<REUSE, V>>: for<'x> Config<
            Store: MultiStore<U> + GetDisjointMut<Multi<U>>,
            Manager<'x> = XManager<'x, Mixed<U>, Exclusive<REUSE, V>>,
            Arena<'x, 'x> = XArena<'x, Mixed<U>, Exclusive<REUSE, V>>,
        >,
{
    #[expect(clippy::mut_from_ref, reason = "mutability is controlled by the handle")]
    pub fn get_disjoint_mut<const N: usize, T>(
        &self,
        handles: [&mut XHandle<'id, T>; N],
    ) -> AResult<[&mut T; N]> {
        Ok(manager!(mut self).get_disjoint_mut(handles)?)
    }
}
impl<'id, U, V> Arena<'id, 'id, Mixed<U>, Exclusive<true, V>>
where
    U: RawBytes,
    GlobalConfig<Mixed<U>, Exclusive<true, V>>: for<'x> Config<
            Store: ReusableMultiStore<U>,
            Manager<'x> = XManager<'x, Mixed<U>, Exclusive<true, V>>,
            Arena<'x, 'x> = XArena<'x, Mixed<U>, Exclusive<true, V>>,
        >,
{
    pub fn remove<T>(&self, handle: XHandle<'id, T>) -> Result<T, (XHandle<'id, T>, ArenaError)> {
        let _guard = self.0.alloc_lock.lock();
        manager!(mut self).remove(handle).map_err(|(handle, err)| (handle, err.into()))
    }
}
